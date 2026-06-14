import os
from pathlib import Path

MAX_FILE_SIZE = 10 * 1024 * 1024
DEFAULT_ENCODING = "utf-8"

class FileProcessor:
    def __init__(self, path: str) -> None:
        self.path = path

    def process(self) -> str:
        return self.read_lines()

    def read_lines(self) -> str:
        with open(self.path) as f:
            return f.read()


def compute_hash(data: str) -> int:
    return hash(data)


def run_pipeline(input_path: str) -> None:
    processor = FileProcessor(input_path)
    content = processor.process()
    result = compute_hash(content)
    print(result)
