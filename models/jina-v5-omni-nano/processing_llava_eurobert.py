"""Custom processor for jina-embeddings-v5-omni-nano.

Keeps Qwen2VL image/video preprocessing (pixel_values, pixel_values_videos,
image_grid_thw, video_grid_thw) and folds both media placeholders into nano's
single `<image>` tokenizer token in the final tokenized output.

Mixed image+video inputs use distinct intermediate markers per modality so
the image- and video-expansion passes don't collide on a shared `<image>`
token — which is the root cause of the upstream Qwen2VLProcessor crash that
walks `while self.image_token in text[i]` and IndexErrors into image_grid_thw
when video placeholders are still in the text.

Two prompt conventions are recognised and disambiguated before expansion:

  1. Proper Qwen placeholders — `<|image_pad|>` / `<|video_pad|>` (optionally
     wrapped in `<|vision_start|>`/`<|vision_end|>`). The pre-replace pass
     maps each to its own modality marker.

  2. Bare `<image>` literals (the legacy convention emitted by `custom_st.py`
     when chat templates collapse `image_token` and `video_token` to the
     same string). Remaining bare `<image>` literals after pass 1 are
     assigned to modality markers in order: as many as are still required
     by `images` first, then `videos`. Anything beyond the matched count
     is left as a literal `<image>` token (preserving the old single-modality
     fallback).

After per-modality expansion both markers collapse to the real `<image>`
token before the tokenizer runs, so input_ids carry exactly the right
number of `<image>` ids in the right positions for masked_scatter to fill
with concatenated image+video features.
"""

import numpy as np

from transformers.feature_extraction_utils import BatchFeature
from transformers.models.qwen2_vl.processing_qwen2_vl import (
    Qwen2VLProcessor,
    Qwen2VLProcessorKwargs,
)


class LlavaEuroBertProcessor(Qwen2VLProcessor):

    _IMG_MARKER = "<__JINA_IMG_PAD__>"
    _VID_MARKER = "<__JINA_VID_PAD__>"

    def __init__(
        self,
        image_processor=None,
        tokenizer=None,
        video_processor=None,
        chat_template=None,
        **kwargs,
    ):
        super().__init__(
            image_processor=image_processor,
            tokenizer=tokenizer,
            video_processor=video_processor,
            chat_template=chat_template,
            **kwargs,
        )
        self.image_token = "<image>"
        self.image_token_id = tokenizer.convert_tokens_to_ids(
            self.image_token
        )
        self.video_token = "<image>"
        self.video_token_id = self.image_token_id

    def __call__(
        self, images=None, text=None, videos=None, **kwargs
    ):
        output_kwargs = self._merge_kwargs(
            Qwen2VLProcessorKwargs,
            tokenizer_init_kwargs=self.tokenizer.init_kwargs,
            **kwargs,
        )

        image_inputs: dict = {}
        videos_inputs: dict = {}
        image_grid_thw = None
        video_grid_thw = None
        if images is not None:
            image_inputs = self.image_processor(
                images=images, **output_kwargs["images_kwargs"]
            )
            image_grid_thw = image_inputs["image_grid_thw"]
        if videos is not None:
            videos_inputs = self.video_processor(
                videos=videos, **output_kwargs["videos_kwargs"]
            )
            video_grid_thw = videos_inputs["video_grid_thw"]

        if text is None:
            return BatchFeature(
                data={**image_inputs, **videos_inputs},
                tensor_type=output_kwargs["text_kwargs"].get("return_tensors"),
            )
        if isinstance(text, str):
            text = [text]
        text = list(text)

        for i in range(len(text)):
            t = text[i]
            t = t.replace(
                "<|vision_start|><|image_pad|><|vision_end|>",
                self._IMG_MARKER,
            )
            t = t.replace(
                "<|vision_start|><|video_pad|><|vision_end|>",
                self._VID_MARKER,
            )
            t = t.replace("<|image_pad|>", self._IMG_MARKER)
            t = t.replace("<|video_pad|>", self._VID_MARKER)
            t = t.replace("<|vision_start|>", "")
            t = t.replace("<|vision_end|>", "")
            text[i] = t

        # Count from the preprocessing output (one grid row per image/video),
        # not from len(images): the latter crashes on a single PIL.Image and
        # mis-counts batched ndarray inputs. grid length is authoritative and
        # matches the number of markers consumed during expansion below.
        n_images = len(image_grid_thw) if image_grid_thw is not None else 0
        n_videos = len(video_grid_thw) if video_grid_thw is not None else 0
        img_markers_in_text = sum(t.count(self._IMG_MARKER) for t in text)
        vid_markers_in_text = sum(t.count(self._VID_MARKER) for t in text)
        images_to_match = max(0, n_images - img_markers_in_text)
        videos_to_match = max(0, n_videos - vid_markers_in_text)

        if images_to_match or videos_to_match:
            for i in range(len(text)):
                if self.image_token not in text[i]:
                    continue
                parts = text[i].split(self.image_token)
                rebuilt = [parts[0]]
                for p in parts[1:]:
                    if images_to_match > 0:
                        rebuilt.append(self._IMG_MARKER)
                        images_to_match -= 1
                    elif videos_to_match > 0:
                        rebuilt.append(self._VID_MARKER)
                        videos_to_match -= 1
                    else:
                        rebuilt.append(self.image_token)
                    rebuilt.append(p)
                text[i] = "".join(rebuilt)

        if images is not None and image_grid_thw is not None:
            merge_length = self.image_processor.merge_size ** 2
            index = 0
            for i in range(len(text)):
                while self._IMG_MARKER in text[i]:
                    n = int(image_grid_thw[index].prod()) // merge_length
                    text[i] = text[i].replace(
                        self._IMG_MARKER, self.image_token * n, 1
                    )
                    index += 1

        if videos is not None and video_grid_thw is not None:
            merge_length = self.video_processor.merge_size ** 2
            index = 0
            for i in range(len(text)):
                while self._VID_MARKER in text[i]:
                    n = int(video_grid_thw[index].prod()) // merge_length
                    text[i] = text[i].replace(
                        self._VID_MARKER, self.video_token * n, 1
                    )
                    index += 1

        return_tensors = output_kwargs["text_kwargs"].pop("return_tensors", None)
        return_mm_token_type_ids = output_kwargs["text_kwargs"].pop(
            "return_mm_token_type_ids", False
        )
        text_inputs = self.tokenizer(
            text, **output_kwargs["text_kwargs"], return_tensors=None
        )
        self._check_special_mm_tokens(
            text, text_inputs, modalities=["image", "video"]
        )

        if return_mm_token_type_ids:
            array_ids = np.array(text_inputs["input_ids"])
            mm_token_type_ids = np.zeros_like(text_inputs["input_ids"])
            mm_token_type_ids[array_ids == self.image_token_id] = 1
            mm_token_type_ids[array_ids == self.video_token_id] = 2
            text_inputs["mm_token_type_ids"] = mm_token_type_ids.tolist()

        return BatchFeature(
            data={**text_inputs, **image_inputs, **videos_inputs},
            tensor_type=return_tensors,
        )
