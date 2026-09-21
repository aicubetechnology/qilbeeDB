"""Durable invalidation queue example; no record bodies, vectors or provider dependencies.

Seed a binding only after reconciling the destination against current authorized
memory. Drain dirty_records separately by refetching current records and treating
404/expiry/access changes as invalidations. This is not a knowledge snapshot.
"""

import json
import sqlite3
from qilbeedb.consumer import VerifiedCursor


class SQLiteInvalidationSink:
    def __init__(self, path):
        self.db = sqlite3.connect(str(path), isolation_level=None)
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("PRAGMA synchronous=FULL")
        self.db.executescript("""
            CREATE TABLE IF NOT EXISTS witnesses (
                binding TEXT PRIMARY KEY, cursor TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS deliveries (
                delivery_id TEXT PRIMARY KEY, binding TEXT NOT NULL, cursor TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS dirty_records (
                binding TEXT NOT NULL, record_id TEXT NOT NULL,
                observed_revision TEXT NOT NULL, kind TEXT NOT NULL,
                PRIMARY KEY (binding, record_id)
            );
        """)

    def close(self):
        self.db.close()

    def load_witness(self, binding_id):
        row = self.db.execute(
            "SELECT cursor FROM witnesses WHERE binding=?", (binding_id,)
        ).fetchone()
        return None if row is None else VerifiedCursor.from_dict(json.loads(row[0]))

    def seed_after_reconciliation(self, binding_id, cursor):
        # An existing witness is deliberately not overwritten by this example.
        self.db.execute(
            "INSERT INTO witnesses VALUES (?, ?)",
            (binding_id, json.dumps(cursor.to_dict(), sort_keys=True)),
        )

    def apply(self, delivery):
        encoded = json.dumps(delivery.cursor.to_dict(), sort_keys=True)
        self.db.execute("BEGIN IMMEDIATE")
        try:
            previous = self.load_witness(delivery.binding_id)
            if previous is None:
                raise ValueError("Reconcile and seed the sink before consuming")
            old = self.db.execute(
                "SELECT binding, cursor FROM deliveries WHERE delivery_id=?",
                (delivery.delivery_id,),
            ).fetchone()
            if old is not None:
                if old != (delivery.binding_id, encoded):
                    raise ValueError("Delivery identity was reused")
            else:
                if (
                    not previous.same_history(delivery.cursor)
                    or delivery.cursor.sequence != previous.sequence + 1
                ):
                    raise ValueError("Nonconsecutive or incompatible delivery")
                self.db.execute(
                    "INSERT INTO deliveries VALUES (?, ?, ?)",
                    (delivery.delivery_id, delivery.binding_id, encoded),
                )
                self.db.execute(
                    """INSERT INTO dirty_records VALUES (?, ?, ?, ?)
                    ON CONFLICT(binding, record_id) DO UPDATE SET
                    observed_revision=excluded.observed_revision, kind=excluded.kind""",
                    (
                        delivery.binding_id,
                        delivery.change["record_id"],
                        str(delivery.change["record_revision"]),
                        delivery.change["kind"],
                    ),
                )
                self.db.execute(
                    "UPDATE witnesses SET cursor=? WHERE binding=?", (encoded, delivery.binding_id)
                )
            self.db.execute("COMMIT")
        except BaseException:
            self.db.execute("ROLLBACK")
            raise
