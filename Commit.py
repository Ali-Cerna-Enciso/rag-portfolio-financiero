"""Create a local Git commit after asking only for its message."""

from pathlib import Path
import subprocess
import sys


REPOSITORY_ROOT = Path(__file__).resolve().parent


def run_git(*arguments: str) -> int:
    """Run Git from the repository containing this script."""
    result = subprocess.run(["git", *arguments], cwd=REPOSITORY_ROOT)
    return result.returncode


def main() -> int:
    message = input("Nombre del commit: ").strip()
    if not message:
        print("El nombre del commit no puede estar vacío.")
        return 1

    add_status = run_git("add", "-A")
    if add_status != 0:
        return add_status

    return run_git("commit", "-m", message)


if __name__ == "__main__":
    sys.exit(main())
