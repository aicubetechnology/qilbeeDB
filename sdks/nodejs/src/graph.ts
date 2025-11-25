import { QilbeeDB } from './client';
import { NodeNotFoundError, QueryError } from './exceptions';
import { QueryResult, QueryStats } from './query';
import { PropertyValue, Properties, Direction } from './types';

export class Node {
  id?: number;
  labels: string[];
  properties: Properties;

  constructor(labels: string[], properties: Properties = {}, id?: number) {
    this.labels = labels;
    this.properties = properties;
    this.id = id;
  }

  get(key: string, defaultValue?: any): any {
    return this.properties[key] !== undefined ? this.properties[key] : defaultValue;
  }

  set(key: string, value: any): void {
    this.properties[key] = value;
  }

  toObject(): any {
    return {
      id: this.id,
      labels: this.labels,
      properties: this.properties
    };
  }
}

export class Relationship {
  id?: number;
  type: string;
  startNode?: number;
  endNode?: number;
  properties: Properties;

  constructor(type: string, startNode: number, endNode: number, properties: Properties = {}, id?: number) {
    this.type = type;
    this.startNode = startNode;
    this.endNode = endNode;
    this.properties = properties;
    this.id = id;
  }

  get(key: string, defaultValue?: any): any {
    return this.properties[key] !== undefined ? this.properties[key] : defaultValue;
  }

  set(key: string, value: any): void {
    this.properties[key] = value;
  }

  toObject(): any {
    return {
      id: this.id,
      type: this.type,
      startNode: this.startNode,
      endNode: this.endNode,
      properties: this.properties
    };
  }
}

export class Graph {
  private name: string;
  private client: QilbeeDB;

  constructor(name: string, client: QilbeeDB) {
    this.name = name;
    this.client = client;
  }

  async createNode(labels: string[], properties: Properties = {}): Promise<Node> {
    const response = await this.client.getAxios().post(`/graphs/${this.name}/nodes`, {
      labels,
      properties
    });
    const data = response.data;
    return new Node(data.labels, data.properties, data.id);
  }

  async getNode(nodeId: number): Promise<Node | null> {
    try {
      const response = await this.client.getAxios().get(`/graphs/${this.name}/nodes/${nodeId}`);
      const data = response.data;
      return new Node(data.labels, data.properties, data.id);
    } catch (error: any) {
      if (error.response && error.response.status === 404) {
        throw new NodeNotFoundError(`Node ${nodeId} not found`);
      }
      throw error;
    }
  }

  async updateNode(node: Node): Promise<Node> {
    const response = await this.client.getAxios().put(`/graphs/${this.name}/nodes/${node.id}`, {
      labels: node.labels,
      properties: node.properties
    });
    const data = response.data;
    return new Node(data.labels, data.properties, data.id);
  }

  async deleteNode(nodeId: number): Promise<boolean> {
    try {
      await this.client.getAxios().delete(`/graphs/${this.name}/nodes/${nodeId}`);
      return true;
    } catch {
      return false;
    }
  }

  async createRelationship(
    fromNode: number | Node,
    relType: string,
    toNode: number | Node,
    properties: Properties = {}
  ): Promise<Relationship> {
    const fromId = typeof fromNode === 'number' ? fromNode : fromNode.id!;
    const toId = typeof toNode === 'number' ? toNode : toNode.id!;

    const response = await this.client.getAxios().post(`/graphs/${this.name}/relationships`, {
      startNode: fromId,
      type: relType,
      endNode: toId,
      properties
    });

    const data = response.data;
    return new Relationship(data.type, data.startNode, data.endNode, data.properties, data.id);
  }

  async findNodes(label?: string, properties?: Properties, limit: number = 100): Promise<Node[]> {
    const params: any = { limit };
    if (label) params.label = label;
    if (properties) params.properties = properties;

    const response = await this.client.getAxios().get(`/graphs/${this.name}/nodes`, { params });
    const data = response.data;

    return (data.nodes || []).map((n: any) => new Node(n.labels, n.properties, n.id));
  }

  async getRelationships(node: number | Node, direction: Direction = 'both'): Promise<Relationship[]> {
    const nodeId = typeof node === 'number' ? node : node.id!;

    const response = await this.client.getAxios().get(
      `/graphs/${this.name}/nodes/${nodeId}/relationships`,
      { params: { direction } }
    );

    const data = response.data;
    return (data.relationships || []).map((r: any) =>
      new Relationship(r.type, r.startNode, r.endNode, r.properties, r.id)
    );
  }

  async query(cypher: string, parameters: Record<string, any> = {}): Promise<QueryResult> {
    try {
      const response = await this.client.getAxios().post(`/graphs/${this.name}/query`, {
        cypher,
        parameters
      });

      const data = response.data;
      const stats: QueryStats = {
        nodesCreated: data.stats.nodesCreated || 0,
        nodesDeleted: data.stats.nodesDeleted || 0,
        relationshipsCreated: data.stats.relationshipsCreated || 0,
        relationshipsDeleted: data.stats.relationshipsDeleted || 0,
        propertiesSet: data.stats.propertiesSet || 0,
        executionTimeMs: data.stats.executionTimeMs || 0
      };

      return new QueryResult(data.results || [], stats);
    } catch (error: any) {
      if (error.response && error.response.status === 400) {
        throw new QueryError(error.response.data.error || 'Query execution failed');
      }
      throw new QueryError(`Query execution failed: ${error.message}`);
    }
  }

  getName(): string {
    return this.name;
  }
}
