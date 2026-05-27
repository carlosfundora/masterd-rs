#!/usr/bin/env python3
# scripts/bootstrap.py
#
# Second stage of the MASTERd recursive bootstrap process.
# Runs inside the installer venv to ensure Rust is installed and vendored
# repositories are checked out before passing control to the Rust orchestrator.
import os
import sys
import subprocess
import shutil
from pathlib import Path

# Color constants for terminal output
RED = "\033[38;5;196m"
GREEN = "\033[38;5;46m"
CYAN = "\033[38;5;51m"
YELLOW = "\033[38;5;226m"
RESET = "\033[0m"

def info(msg: str):
    print(f"{CYAN}[bootstrap-py]{RESET} {msg}", flush=True)

def success(msg: str):
    print(f"{GREEN}[bootstrap-py]{RESET} {msg}", flush=True)

def warn(msg: str):
    print(f"{YELLOW}[bootstrap-py]{RESET} {msg}", flush=True)

def error(msg: str):
    print(f"{RED}[bootstrap-py] ERROR:{RESET} {msg}", file=sys.stderr, flush=True)

def run_cmd(cmd: list, cwd: Path = None, env: dict = None) -> bool:
    try:
        res = subprocess.run(cmd, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        if res.returncode != 0:
            warn(f"Command {' '.join(cmd)} failed with exit code {res.returncode}")
            if res.stderr:
                warn(f"Stderr: {res.stderr.strip()}")
            return False
        return True
    except Exception as e:
        warn(f"Failed to run {' '.join(cmd)}: {e}")
        return False

def ensure_git() -> bool:
    if shutil.which("git"):
        return True
    error("git is required for bootstrapping vendored dependencies.")
    return False

def clone_vendor(name: str, url: str, vendor_root: Path):
    dest = vendor_root / name
    git_dir = dest / ".git"

    if dest.exists() and not git_dir.exists():
        info(f"Removing failed/corrupted vendor install: {name}")
        shutil.rmtree(dest, ignore_errors=True)

    if git_dir.exists():
        info(f"Vendor {name} already exists, skipping.")
        return

    info(f"Cloning {name} from {url}...")
    try:
        subprocess.run(["git", "clone", "--depth", "1", url, str(dest)], check=True)
        success(f"Successfully cloned {name}")
    except subprocess.CalledProcessError as e:
        error(f"Failed to clone {name}: {e}")
        sys.exit(1)

def ensure_rust() -> str:
    # 1. Check if rustc is already in path
    rustc_bin = shutil.which("rustc")
    if rustc_bin:
        return rustc_bin

    # 2. Check if it's in the default ~/.cargo/bin location
    home = Path.home()
    cargo_bin_dir = home / ".cargo" / "bin"
    rustc_fallback = cargo_bin_dir / "rustc"
    if rustc_fallback.exists() and os.access(rustc_fallback, os.X_OK):
        os.environ["PATH"] = f"{cargo_bin_dir}:{os.environ.get('PATH', '')}"
        success(f"Added Cargo bin path {cargo_bin_dir} to PATH")
        return str(rustc_fallback)

    # 3. Rust not found; attempt to install rustup
    info("Rust toolchain not found. Initiating rustup installation...")
    import urllib.request
    rustup_sh = Path("/tmp/rustup.sh")
    try:
        urllib.request.urlretrieve("https://sh.rustup.rs", rustup_sh)
        rustup_sh.chmod(0o755)
        info("Running rustup installer...")
        subprocess.run([str(rustup_sh), "-y", "--profile", "minimal", "--default-toolchain", "stable"], check=True)
        # Prepend new cargo path
        os.environ["PATH"] = f"{cargo_bin_dir}:{os.environ.get('PATH', '')}"
        rustc_path = cargo_bin_dir / "rustc"
        if rustc_path.exists():
            success("Rust toolchain bootstrapped successfully.")
            return str(rustc_path)
    except Exception as e:
        error(f"Failed to install Rust via rustup: {e}")
        sys.exit(1)
    finally:
        if rustup_sh.exists():
            rustup_sh.unlink()

    error("Rust installation finished but 'rustc' is still missing.")
    sys.exit(1)

def main():
    script_dir = Path(__file__).resolve().parent
    root_dir = script_dir.parent
    vendor_root = root_dir / "vendor"
    vendor_root.mkdir(parents=True, exist_ok=True)

    info(f"Starting Python bootstrap stage. Root: {root_dir}")

    # Ensure git is present
    if not ensure_git():
        sys.exit(1)

    # Clone all vendored packages so Cargo registry matches
    info("Ensuring vendored dependencies are present...")
    clone_vendor("candle", "https://github.com/huggingface/candle.git", vendor_root)
    clone_vendor("tokenizers", "https://github.com/huggingface/tokenizers.git", vendor_root)
    clone_vendor("tauri", "https://github.com/tauri-apps/tauri.git", vendor_root)
    clone_vendor("lopdf", "https://github.com/J-F-Liu/lopdf.git", vendor_root)
    clone_vendor("iced", "https://github.com/iced-rs/iced.git", vendor_root)
    
    # Handle tesseract-rs with secondary fallback
    tesseract_dest = vendor_root / "tesseract-rs"
    if tesseract_dest.exists() and not (tesseract_dest / ".git").exists():
        shutil.rmtree(tesseract_dest, ignore_errors=True)
    if not (tesseract_dest / ".git").exists():
        info("Cloning tesseract-rs...")
        try:
            subprocess.run(["git", "clone", "--depth", "1", "https://github.com/cafercangundogdu/tesseract-rs.git", str(tesseract_dest)], check=True)
        except Exception:
            warn("Primary tesseract-rs repository failed. Retrying with fallback...")
            subprocess.run(["git", "clone", "--depth", "1", "https://github.com/antimatter15/tesseract-rs.git", str(tesseract_dest)], check=True)

    # Ensure Rust is ready
    ensure_rust()

    # Pass execution control to the Rust bootstrap orchestrator
    info("Rust toolchain verified. Launching Rust orchestrator...")
    cargo_bin = shutil.which("cargo")
    if not cargo_bin:
        error("cargo executable not found in PATH.")
        sys.exit(1)

    # Run the cargo bootstrap process
    cmd = [cargo_bin, "run", "--manifest-path", str(root_dir / "apps/masterd-bootstrap/Cargo.toml"), "--", "--install"]
    info(f"Executing: {' '.join(cmd)}")
    
    try:
        # We inherit stdout and stderr directly so the user gets real-time rust logs
        res = subprocess.run(cmd, cwd=root_dir)
        sys.exit(res.returncode)
    except KeyboardInterrupt:
        warn("Bootstrap interrupted by user.")
        sys.exit(130)
    except Exception as e:
        error(f"Failed to execute Rust orchestrator: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main()
