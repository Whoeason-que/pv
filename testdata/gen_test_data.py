"""Generate test parquet files for the parquet viewer TUI app."""

import os
import random
from datetime import datetime, timedelta

import pyarrow as pa
import pyarrow.parquet as pq

random.seed(42)

HERE = os.path.dirname(os.path.abspath(__file__))


def gen_basic_types():
    n = 1000
    ids = list(range(1, n + 1))
    names = [f"user_{i:03d}" for i in range(1, n + 1)]
    ages = [random.randint(18, 80) for _ in range(n)]
    scores = [random.uniform(0.0, 100.0) for _ in range(n)]
    is_active = [random.choice([True, False]) for _ in range(n)]
    start = datetime(2023, 1, 1)
    created_at = [
        start + timedelta(seconds=random.randint(0, int((datetime(2024, 12, 31) - start).total_seconds())))
        for _ in range(n)
    ]

    table = pa.table(
        {
            "id": pa.array(ids, type=pa.int64()),
            "name": pa.array(names, type=pa.string()),
            "age": pa.array(ages, type=pa.int32()),
            "score": pa.array(scores, type=pa.float64()),
            "is_active": pa.array(is_active, type=pa.bool_()),
            "created_at": pa.array(created_at, type=pa.timestamp("us")),
        }
    )
    pq.write_table(table, os.path.join(HERE, "basic_types.parquet"))
    print(f"basic_types.parquet: {n} rows")


def gen_nested_types():
    n = 100
    ids = list(range(1, n + 1))
    tag_pool = ["tag1", "tag2", "tag3", "tag4", "tag5"]
    tags = [random.sample(tag_pool, k=random.randint(1, 3)) for _ in range(n)]
    info = [
        {"city": random.choice(["NYC", "LA", "SF", "Chicago", "Boston"]), "zip": random.randint(10000, 99999)}
        for _ in range(n)
    ]
    scores = [[random.randint(0, 100) for _ in range(random.randint(2, 5))] for _ in range(n)]
    metadata = [
        {f"key_{i}": f"val_{random.randint(0, 99)}" for i in range(random.randint(1, 3))}
        for _ in range(n)
    ]

    table = pa.table(
        {
            "id": pa.array(ids, type=pa.int64()),
            "tags": pa.array(tags, type=pa.list_(pa.string())),
            "info": pa.array(info, type=pa.struct([("city", pa.string()), ("zip", pa.int32())])),
            "scores": pa.array(scores, type=pa.list_(pa.int32())),
            "metadata": pa.array(metadata, type=pa.map_(pa.string(), pa.string())),
        }
    )
    pq.write_table(table, os.path.join(HERE, "nested_types.parquet"))
    print(f"nested_types.parquet: {n} rows")


def gen_nullable_types():
    n = 200
    ids = list(range(1, n + 1))

    def maybe(gen, null_rate=0.3):
        return None if random.random() < null_rate else gen()

    names = [maybe(lambda: f"name_{random.randint(1000, 9999)}") for _ in range(n)]
    values = [maybe(lambda: round(random.uniform(-1000.0, 1000.0), 4)) for _ in range(n)]
    descriptions = [maybe(lambda: f"desc {random.randint(1, 500)}") for _ in range(n)]

    table = pa.table(
        {
            "id": pa.array(ids, type=pa.int64()),
            "name": pa.array(names, type=pa.string()),
            "value": pa.array(values, type=pa.float64()),
            "description": pa.array(descriptions, type=pa.string()),
        }
    )
    pq.write_table(table, os.path.join(HERE, "nullable_types.parquet"))
    print(f"nullable_types.parquet: {n} rows")


def gen_large_file():
    n = 100000
    ids = list(range(1, n + 1))
    categories = [random.choice(["A", "B", "C", "D", "E"]) for _ in range(n)]
    amounts = [round(random.uniform(0.0, 100000.0), 2) for _ in range(n)]
    start = datetime(2020, 1, 1)
    timestamps = [
        start + timedelta(seconds=random.randint(0, int((datetime(2025, 12, 31) - start).total_seconds())))
        for _ in range(n)
    ]

    table = pa.table(
        {
            "id": pa.array(ids, type=pa.int64()),
            "category": pa.array(categories, type=pa.string()),
            "amount": pa.array(amounts, type=pa.float64()),
            "timestamp": pa.array(timestamps, type=pa.timestamp("us")),
        }
    )
    pq.write_table(table, os.path.join(HERE, "large_file.parquet"))
    print(f"large_file.parquet: {n} rows")


def gen_partitioned():
    n = 500
    base = os.path.join(HERE, "partitioned")
    specs = [
        (2023, "01"),
        (2023, "02"),
        (2024, "01"),
    ]
    for year, month in specs:
        ids = list(range(1, n + 1))
        amounts = [round(random.uniform(0.0, 1000.0), 2) for _ in range(n)]
        statuses = [random.choice(["pending", "completed", "failed", "refunded"]) for _ in range(n)]

        table = pa.table(
            {
                "id": pa.array(ids, type=pa.int64()),
                "amount": pa.array(amounts, type=pa.float64()),
                "status": pa.array(statuses, type=pa.string()),
                "year": pa.array([year] * n, type=pa.int32()),
                "month": pa.array([int(month)] * n, type=pa.int32()),
            }
        )
        table = table.drop(["year", "month"])

        out_dir = os.path.join(base, f"year={year}", f"month={month}")
        os.makedirs(out_dir, exist_ok=True)
        pq.write_table(table, os.path.join(out_dir, "data.parquet"))
        print(f"partitioned/year={year}/month={month}/data.parquet: {n} rows")


def main():
    gen_basic_types()
    gen_nested_types()
    gen_nullable_types()
    gen_large_file()
    gen_partitioned()
    print("\nAll test parquet files generated.")


if __name__ == "__main__":
    main()
