/**
 * Common types and interfaces.
 */

export type PropertyValue = string | number | boolean | null;

export interface Properties {
  [key: string]: PropertyValue;
}

export type Direction = 'incoming' | 'outgoing' | 'both';
