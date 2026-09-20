# Transactions

Rust storage transactions publish node, relationship and index mutations in one
atomic RocksDB batch. Direct entity writes use the same index maintenance path.
Transaction reads are cached per entity; snapshot isolation and conflict
detection are not implemented.

See the [atomic commit contract](../architecture/atomic-commits.md) for write
semantics, durability options, tested behavior and known limitations. Separate
HTTP requests are not automatically grouped into a storage transaction.

## Next Steps

- Learn about [Nodes](nodes.md)
- Explore [Relationships](relationships.md)
- Read the [Python SDK](../client-libraries/python.md)
