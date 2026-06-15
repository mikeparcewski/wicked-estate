import json
import hashlib
from typing import List, Optional

MAX_BATCH_SIZE = 500
DEFAULT_TIMEOUT = 30

class RecordStore:
    def __init__(self, name: str) -> None:
        self.name = name
        self._records: List[dict] = []

    def insert(self, record: dict) -> None:
        validated = validate_record(record)
        self._records.append(validated)

    def find(self, key: str) -> Optional[dict]:
        for rec in self._records:
            if rec.get('id') == key:
                return rec
        return None

    def flush(self) -> str:
        return serialize_batch(self._records)


def validate_record(record: dict) -> dict:
    if 'id' not in record:
        raise ValueError("record must have an id")
    return record


def serialize_batch(records: List[dict]) -> str:
    payload = json.dumps(records)
    digest = hashlib.sha256(payload.encode()).hexdigest()
    return f"{digest}:{payload}"
