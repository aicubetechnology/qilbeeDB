import { QilbeeDB } from './client';
import { MemoryError } from './exceptions';

export interface MemoryConfig {
  maxEpisodes?: number;
  minRelevance?: number;
  autoConsolidate?: boolean;
  autoForget?: boolean;
  consolidationThreshold?: number;
  episodicRetentionDays?: number;
}

export interface MemoryStatistics {
  totalEpisodes: number;
  oldestEpisode?: number;
  newestEpisode?: number;
  avgRelevance: number;
}

export class Episode {
  agentId: string;
  episodeType: string;
  content: any;
  eventTime: number;
  metadata?: Record<string, any>;

  constructor(
    agentId: string,
    episodeType: string,
    content: any,
    eventTime?: number,
    metadata?: Record<string, any>
  ) {
    this.agentId = agentId;
    this.episodeType = episodeType;
    this.content = content;
    this.eventTime = eventTime || Math.floor(Date.now() / 1000);
    this.metadata = metadata;
  }

  static conversation(
    agentId: string,
    userInput: string,
    agentResponse: string,
    metadata?: Record<string, any>
  ): Episode {
    return new Episode(
      agentId,
      'conversation',
      { userInput, agentResponse },
      undefined,
      metadata
    );
  }

  static observation(agentId: string, observation: string, metadata?: Record<string, any>): Episode {
    return new Episode(agentId, 'observation', { observation }, undefined, metadata);
  }

  static action(agentId: string, action: string, result: string, metadata?: Record<string, any>): Episode {
    return new Episode(agentId, 'action', { action, result }, undefined, metadata);
  }

  toObject(): any {
    return {
      agentId: this.agentId,
      episodeType: this.episodeType,
      content: this.content,
      eventTime: this.eventTime,
      metadata: this.metadata
    };
  }
}

export class AgentMemory {
  private agentId: string;
  private client: QilbeeDB;
  private config: MemoryConfig;

  constructor(agentId: string, client: QilbeeDB, config?: Partial<MemoryConfig>) {
    this.agentId = agentId;
    this.client = client;
    this.config = {
      maxEpisodes: config?.maxEpisodes || 10000,
      minRelevance: config?.minRelevance || 0.1,
      autoConsolidate: config?.autoConsolidate || false,
      autoForget: config?.autoForget || false,
      consolidationThreshold: config?.consolidationThreshold || 5000,
      episodicRetentionDays: config?.episodicRetentionDays || 30
    };
  }

  async storeEpisode(episode: Episode): Promise<string> {
    try {
      const response = await this.client.getAxios().post(
        `/memory/${this.agentId}/episodes`,
        episode.toObject()
      );
      return response.data.episodeId;
    } catch (error: any) {
      throw new MemoryError(`Failed to store episode: ${error.message}`);
    }
  }

  async getEpisode(episodeId: string): Promise<Episode | null> {
    try {
      const response = await this.client.getAxios().get(
        `/memory/${this.agentId}/episodes/${episodeId}`
      );
      const data = response.data;
      return new Episode(data.agentId, data.episodeType, data.content, data.eventTime);
    } catch {
      return null;
    }
  }

  async getRecentEpisodes(limit: number = 10): Promise<Episode[]> {
    const response = await this.client.getAxios().get(
      `/memory/${this.agentId}/episodes/recent`,
      { params: { limit } }
    );

    return (response.data.episodes || []).map((e: any) =>
      new Episode(e.agentId, e.episodeType, e.content, e.eventTime)
    );
  }

  async searchEpisodes(query: string, limit: number = 10): Promise<Episode[]> {
    const response = await this.client.getAxios().post(
      `/memory/${this.agentId}/episodes/search`,
      { query, limit }
    );

    return (response.data.episodes || []).map((e: any) =>
      new Episode(e.agentId, e.episodeType, e.content, e.eventTime)
    );
  }

  async getStatistics(): Promise<MemoryStatistics> {
    const response = await this.client.getAxios().get(`/memory/${this.agentId}/statistics`);
    return response.data;
  }

  async consolidate(): Promise<number> {
    const response = await this.client.getAxios().post(`/memory/${this.agentId}/consolidate`);
    return response.data.consolidated;
  }

  async forget(minRelevance: number = 0.1): Promise<number> {
    const response = await this.client.getAxios().post(`/memory/${this.agentId}/forget`, {
      minRelevance
    });
    return response.data.forgotten;
  }

  async clear(): Promise<boolean> {
    try {
      await this.client.getAxios().delete(`/memory/${this.agentId}`);
      return true;
    } catch {
      return false;
    }
  }

  getAgentId(): string {
    return this.agentId;
  }
}
