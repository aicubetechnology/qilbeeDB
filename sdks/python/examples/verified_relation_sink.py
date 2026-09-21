"""Durable scoped graph invalidation example; never treat event metadata as truth.

Reconcile and seed explicitly. After processing an invalidation, conditionally
acknowledge its exact cursor so a concurrent new event cannot be cleared. Observe
the memory stream too and revalidate current graph eligibility before reuse.
"""

import json
import sqlite3
from qilbeedb import RelationCursor


class SQLiteRelationInvalidationSink:
    def __init__(self, path):
        self.db = sqlite3.connect(str(path), isolation_level=None)
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("PRAGMA synchronous=FULL")
        self.db.executescript("""
            CREATE TABLE IF NOT EXISTS relation_witnesses (
                binding TEXT PRIMARY KEY, cursor TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS relation_deliveries (
                delivery_id TEXT PRIMARY KEY, binding TEXT NOT NULL, cursor TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS dirty_graph_bindings (
                binding TEXT PRIMARY KEY, cursor TEXT NOT NULL
            );
        """)

    def close(self):
        self.db.close()

    def load_witness(self, binding_id):
        row = self.db.execute(
            "SELECT cursor FROM relation_witnesses WHERE binding=?", (binding_id,)
        ).fetchone()
        return None if row is None else RelationCursor.from_dict(json.loads(row[0]))

    def seed_after_reconciliation(self, binding_id, cursor):
        if not isinstance(cursor, RelationCursor):
            raise TypeError("A complete relation cursor is required")
        self.db.execute(
            "INSERT INTO relation_witnesses VALUES (?, ?)",
            (binding_id, json.dumps(cursor.to_dict(), sort_keys=True)),
        )

    def pending_invalidation(self, binding_id):
        row = self.db.execute(
            "SELECT cursor FROM dirty_graph_bindings WHERE binding=?", (binding_id,)
        ).fetchone()
        return None if row is None else RelationCursor.from_dict(json.loads(row[0]))

    def acknowledge_invalidation(self, binding_id, observed_cursor):
        """Clear only the observed invalidation after the worker's effects are durable."""
        if not isinstance(observed_cursor, RelationCursor):
            raise TypeError("A complete relation cursor is required")
        return (
            self.db.execute(
                "DELETE FROM dirty_graph_bindings WHERE binding=? AND cursor=?",
                (binding_id, json.dumps(observed_cursor.to_dict(), sort_keys=True)),
            ).rowcount
            == 1
        )

    def apply(self, delivery):
        encoded = json.dumps(delivery.cursor.to_dict(), sort_keys=True)
        self.db.execute("BEGIN IMMEDIATE")
        try:
            previous = self.load_witness(delivery.binding_id)
            if previous is None:
                raise ValueError("Reconcile and seed the sink before consuming")
            old = self.db.execute(
                "SELECT binding, cursor FROM relation_deliveries WHERE delivery_id=?",
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
                    "INSERT INTO relation_deliveries VALUES (?, ?, ?)",
                    (delivery.delivery_id, delivery.binding_id, encoded),
                )
                self.db.execute(
                    """INSERT INTO dirty_graph_bindings VALUES (?, ?)
                    ON CONFLICT(binding) DO UPDATE SET cursor=excluded.cursor""",
                    (delivery.binding_id, encoded),
                )
                self.db.execute(
                    "UPDATE relation_witnesses SET cursor=? WHERE binding=?",
                    (encoded, delivery.binding_id),
                )
            self.db.execute("COMMIT")
        except BaseException:
            self.db.execute("ROLLBACK")
            raise
