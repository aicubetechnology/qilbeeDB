# @qilbeedb/client

Official Node.js/TypeScript client library for QilbeeDB - Enterprise Graph Database with Bi-Temporal Agent Memory.

## Installation

```bash
npm install @qilbeedb/client
# or
yarn add @qilbeedb/client
# or
pnpm add @qilbeedb/client
```

## Quick Start

### Basic Graph Operations

```typescript
import { QilbeeDB } from '@qilbeedb/client';

// Connect to database
const db = new QilbeeDB('http://localhost:7474');

// Get or create a graph
const graph = await db.graph('social');

// Create nodes
const alice = await graph.createNode(
  ['Person'],
  { name: 'Alice', age: 30 }
);

const bob = await graph.createNode(
  ['Person'],
  { name: 'Bob', age: 35 }
);

// Create relationship
const knows = await graph.createRelationship(
  alice.id,
  'KNOWS',
  bob.id,
  { since: 2020 }
);

// Query nodes
const people = await graph.findNodes('Person');
for (const person of people) {
  console.log(`${person.get('name')} is ${person.get('age')} years old`);
}
```

### Cypher Queries

```typescript
// Execute Cypher query
const result = await graph.query(
  'MATCH (p:Person) WHERE p.age > $age RETURN p.name, p.age ORDER BY p.age DESC',
  { age: 25 }
);

for (const record of result) {
  console.log(`${record['p.name']}: ${record['p.age']}`);
}

// Query statistics
console.log(`Execution time: ${result.stats.executionTimeMs}ms`);
```

### Query Builder

```typescript
import { Query } from '@qilbeedb/client';

// Build query fluently
const result = await new Query(graph)
  .match('(p:Person)')
  .where('p.age > $age', { age: 25 })
  .returnClause('p.name', 'p.age')
  .orderBy('p.age', true)
  .limit(10)
  .execute();
```

### Agent Memory

```typescript
import { Episode } from '@qilbeedb/client';

// Create agent memory
const memory = db.agentMemory('agent-001', {
  maxEpisodes: 10000,
  minRelevance: 0.1
});

// Store conversation
const episode = Episode.conversation(
  'agent-001',
  'What is the capital of France?',
  'The capital of France is Paris.'
);
await memory.storeEpisode(episode);

// Store observation
const obs = Episode.observation(
  'agent-001',
  'User seems interested in European geography'
);
await memory.storeEpisode(obs);

// Retrieve recent memories
const recent = await memory.getRecentEpisodes(10);
for (const ep of recent) {
  console.log(ep.content);
}

// Search memories
const results = await memory.searchEpisodes('France');

// Get statistics
const stats = await memory.getStatistics();
console.log(`Total episodes: ${stats.totalEpisodes}`);
console.log(`Average relevance: ${stats.avgRelevance.toFixed(2)}`);

// Consolidate and forget
await memory.consolidate();
await memory.forget(0.2);
```

### TypeScript Support

Full TypeScript support with type definitions:

```typescript
import { QilbeeDB, Node, Relationship, Episode } from '@qilbeedb/client';

const db = new QilbeeDB({
  uri: 'http://localhost:7474',
  auth: {
    username: 'neo4j',
    password: 'password'
  },
  timeout: 30000
});

// Type-safe operations
const graph = await db.graph('social');
const node: Node = await graph.createNode(['Person'], { name: 'Alice' });
console.log(node.get('name')); // TypeScript knows this returns any
```

### Error Handling

```typescript
import {
  QilbeeDBError,
  ConnectionError,
  QueryError,
  AuthenticationError
} from '@qilbeedb/client';

try {
  const result = await graph.query('INVALID QUERY');
} catch (error) {
  if (error instanceof QueryError) {
    console.error(`Query failed: ${error.message}`);
  } else if (error instanceof ConnectionError) {
    console.error(`Connection failed: ${error.message}`);
  } else if (error instanceof QilbeeDBError) {
    console.error(`Database error: ${error.message}`);
  }
}
```

## API Reference

### QilbeeDB

Main database client.

```typescript
class QilbeeDB {
  constructor(options: string | QilbeeDBOptions);

  graph(name: string): Promise<Graph>;
  listGraphs(): Promise<string[]>;
  createGraph(name: string): Promise<Graph>;
  deleteGraph(name: string): Promise<boolean>;
  health(): Promise<HealthStatus>;
  agentMemory(agentId: string, config?: Partial<MemoryConfig>): AgentMemory;
  close(): Promise<void>;
}
```

### Graph

Graph operations.

```typescript
class Graph {
  query(cypher: string, parameters?: Record<string, any>): Promise<QueryResult>;
  createNode(labels: string[], properties?: PropertyValue): Promise<Node>;
  getNode(nodeId: number): Promise<Node | null>;
  updateNode(node: Node): Promise<Node>;
  deleteNode(nodeId: number): Promise<boolean>;
  createRelationship(
    fromNode: number | Node,
    relType: string,
    toNode: number | Node,
    properties?: PropertyValue
  ): Promise<Relationship>;
  findNodes(label?: string, properties?: PropertyValue, limit?: number): Promise<Node[]>;
  getRelationships(node: number | Node, direction?: Direction): Promise<Relationship[]>;
  getName(): string;
}
```

### Node

Graph node.

```typescript
class Node {
  id?: number;
  labels: string[];
  properties: PropertyValue;

  get(key: string, defaultValue?: any): any;
  set(key: string, value: any): void;
  toObject(): any;
}
```

### Relationship

Graph relationship.

```typescript
class Relationship {
  id?: number;
  type: string;
  startNode?: number;
  endNode?: number;
  properties: PropertyValue;

  get(key: string, defaultValue?: any): any;
  set(key: string, value: any): void;
  toObject(): any;
}
```

### AgentMemory

Bi-temporal agent memory.

```typescript
class AgentMemory {
  storeEpisode(episode: Episode): Promise<string>;
  getEpisode(episodeId: string): Promise<Episode | null>;
  getRecentEpisodes(limit?: number): Promise<Episode[]>;
  searchEpisodes(query: string, limit?: number): Promise<Episode[]>;
  getStatistics(): Promise<MemoryStatistics>;
  consolidate(): Promise<number>;
  forget(minRelevance?: number): Promise<number>;
  clear(): Promise<boolean>;
  getAgentId(): string;
}
```

### Episode

Episodic memory.

```typescript
class Episode {
  static conversation(
    agentId: string,
    userInput: string,
    agentResponse: string,
    metadata?: Record<string, any>
  ): Episode;

  static observation(
    agentId: string,
    observation: string,
    metadata?: Record<string, any>
  ): Episode;

  static action(
    agentId: string,
    action: string,
    result: string,
    metadata?: Record<string, any>
  ): Episode;

  toObject(): any;
}
```

## Configuration

### Connection Options

```typescript
const db = new QilbeeDB({
  uri: 'http://localhost:7474',
  auth: {
    username: 'username',
    password: 'password'
  },
  timeout: 30000,
  verifySsl: true
});
```

### Memory Configuration

```typescript
const memory = db.agentMemory('agent-001', {
  maxEpisodes: 10000,
  minRelevance: 0.1,
  autoConsolidate: true,
  autoForget: true,
  consolidationThreshold: 5000,
  episodicRetentionDays: 30
});
```

## Examples

### Complete Example

```typescript
import { QilbeeDB, Episode, Query } from '@qilbeedb/client';

async function main() {
  // Connect
  const db = new QilbeeDB('http://localhost:7474');

  // Graph operations
  const graph = await db.graph('social');

  const alice = await graph.createNode(['Person'], {
    name: 'Alice',
    age: 30,
    city: 'Paris'
  });

  const bob = await graph.createNode(['Person'], {
    name: 'Bob',
    age: 35,
    city: 'London'
  });

  await graph.createRelationship(alice.id!, 'KNOWS', bob.id!, {
    since: 2020
  });

  // Query with builder
  const result = await new Query(graph)
    .match('(p:Person)')
    .where('p.age > $age', { age: 25 })
    .returnClause('p.name', 'p.age', 'p.city')
    .orderBy('p.age', true)
    .execute();

  console.log(`Found ${result.length} people`);

  // Agent memory
  const memory = db.agentMemory('agent-001');

  await memory.storeEpisode(
    Episode.conversation(
      'agent-001',
      'Tell me about Alice',
      'Alice is 30 years old and lives in Paris'
    )
  );

  const stats = await memory.getStatistics();
  console.log(`Memory: ${stats.totalEpisodes} episodes`);

  await db.close();
}

main().catch(console.error);
```

## Development

### Building

```bash
npm install
npm run build
```

### Testing

```bash
npm test
npm run test:watch
```

### Linting

```bash
npm run lint
npm run format
```

## License

Apache License 2.0

## Support

- Documentation: https://docs.qilbeedb.com
- Issues: https://github.com/your-org/qilbeedb/issues
- Email: support@qilbeedb.com
