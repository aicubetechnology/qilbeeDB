import axios, { AxiosInstance } from 'axios';
import { Graph } from './graph';
import { AgentMemory, MemoryConfig } from './memory';
import { ConnectionError, AuthenticationError } from './exceptions';

export interface QilbeeDBOptions {
  uri: string;
  auth?: {
    username: string;
    password: string;
  };
  timeout?: number;
  verifySsl?: boolean;
}

export interface HealthStatus {
  status: string;
  version?: string;
}

export class QilbeeDB {
  private baseUrl: string;
  private axios: AxiosInstance;

  constructor(uriOrOptions: string | QilbeeDBOptions) {
    if (typeof uriOrOptions === 'string') {
      this.baseUrl = uriOrOptions;
      this.axios = axios.create({
        baseURL: this.baseUrl,
        timeout: 30000
      });
    } else {
      this.baseUrl = uriOrOptions.uri;
      const axiosConfig: any = {
        baseURL: this.baseUrl,
        timeout: uriOrOptions.timeout || 30000
      };

      if (uriOrOptions.auth) {
        axiosConfig.auth = {
          username: uriOrOptions.auth.username,
          password: uriOrOptions.auth.password
        };
      }

      this.axios = axios.create(axiosConfig);
    }
  }

  async health(): Promise<HealthStatus> {
    try {
      const response = await this.axios.get('/health');
      return response.data;
    } catch (error: any) {
      if (error.response && error.response.status === 401) {
        throw new AuthenticationError('Authentication failed');
      }
      throw new ConnectionError(`Failed to connect: ${error.message}`);
    }
  }

  async graph(name: string): Promise<Graph> {
    return new Graph(name, this);
  }

  async listGraphs(): Promise<string[]> {
    const response = await this.axios.get('/graphs');
    return response.data.graphs || [];
  }

  async createGraph(name: string): Promise<Graph> {
    await this.axios.post(`/graphs/${name}`);
    return new Graph(name, this);
  }

  async deleteGraph(name: string): Promise<boolean> {
    try {
      await this.axios.delete(`/graphs/${name}`);
      return true;
    } catch {
      return false;
    }
  }

  agentMemory(agentId: string, config?: Partial<MemoryConfig>): AgentMemory {
    return new AgentMemory(agentId, this, config);
  }

  async close(): Promise<void> {
    // No persistent connection to close in HTTP client
  }

  getAxios(): AxiosInstance {
    return this.axios;
  }
}
