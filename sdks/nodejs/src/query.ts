import { Graph } from './graph';

export interface QueryStats {
  nodesCreated: number;
  nodesDeleted: number;
  relationshipsCreated: number;
  relationshipsDeleted: number;
  propertiesSet: number;
  executionTimeMs: number;
}

export class QueryResult {
  private results: Record<string, any>[];
  stats: QueryStats;

  constructor(results: Record<string, any>[], stats: QueryStats) {
    this.results = results;
    this.stats = stats;
  }

  get length(): number {
    return this.results.length;
  }

  *[Symbol.iterator]() {
    for (const result of this.results) {
      yield result;
    }
  }

  map<T>(fn: (result: Record<string, any>) => T): T[] {
    return this.results.map(fn);
  }

  [index: number]: Record<string, any>;
}

// Add array indexing support
Object.defineProperty(QueryResult.prototype, Symbol.iterator, {
  *value() {
    for (const result of this.results) {
      yield result;
    }
  }
});

// Proxy for array-like indexing
export function createQueryResult(results: Record<string, any>[], stats: QueryStats): QueryResult {
  const qr = new QueryResult(results, stats);
  return new Proxy(qr, {
    get(target, prop) {
      if (typeof prop === 'string') {
        const index = parseInt(prop, 10);
        if (!isNaN(index)) {
          return target['results'][index];
        }
      }
      return (target as any)[prop];
    }
  });
}

interface QueryPart {
  match: string[];
  where: string[];
  return: string[];
  orderBy: Array<{ field: string; desc: boolean }>;
  limit?: number;
  skip?: number;
}

export class Query {
  private graph: Graph;
  private queryParts: QueryPart;
  private _parameters: Record<string, any>;

  constructor(graph: Graph) {
    this.graph = graph;
    this.queryParts = {
      match: [],
      where: [],
      return: [],
      orderBy: []
    };
    this._parameters = {};
  }

  match(pattern: string): this {
    this.queryParts.match.push(pattern);
    return this;
  }

  where(condition: string, params?: Record<string, any>): this {
    this.queryParts.where.push(condition);
    if (params) {
      Object.assign(this._parameters, params);
    }
    return this;
  }

  returnClause(...fields: string[]): this {
    this.queryParts.return.push(...fields);
    return this;
  }

  orderBy(field: string, desc: boolean = false): this {
    this.queryParts.orderBy.push({ field, desc });
    return this;
  }

  limit(limit: number): this {
    this.queryParts.limit = limit;
    return this;
  }

  skip(skip: number): this {
    this.queryParts.skip = skip;
    return this;
  }

  build(): string {
    const parts: string[] = [];

    // MATCH
    this.queryParts.match.forEach(m => parts.push(`MATCH ${m}`));

    // WHERE
    if (this.queryParts.where.length > 0) {
      parts.push(`WHERE ${this.queryParts.where.join(' AND ')}`);
    }

    // RETURN
    if (this.queryParts.return.length > 0) {
      parts.push(`RETURN ${this.queryParts.return.join(', ')}`);
    }

    // ORDER BY
    if (this.queryParts.orderBy.length > 0) {
      const orderStrs = this.queryParts.orderBy.map(
        o => `${o.field} ${o.desc ? 'DESC' : 'ASC'}`
      );
      parts.push(`ORDER BY ${orderStrs.join(', ')}`);
    }

    // SKIP
    if (this.queryParts.skip !== undefined) {
      parts.push(`SKIP ${this.queryParts.skip}`);
    }

    // LIMIT
    if (this.queryParts.limit !== undefined) {
      parts.push(`LIMIT ${this.queryParts.limit}`);
    }

    return parts.join(' ');
  }

  async execute(): Promise<QueryResult> {
    const cypher = this.build();
    return this.graph.query(cypher, this._parameters);
  }
}
