import { EventEmitter } from 'events';
import { readFileSync } from 'fs';

export interface Processor {
    process(data: string): string;
}

export enum Status {
    Pending,
    Running,
    Done,
}

export type ProcessorId = string;
export type Callback = (result: string) => void;

export const MAX_RETRIES: number = 3;
export const DEFAULT_TIMEOUT: number = 5000;

export const createEmitter = (): EventEmitter => new EventEmitter();

let retryCount = 0;

export class DataPipeline implements Processor {
    private emitter: EventEmitter;
    private readonly name: string = "pipeline";

    constructor() {
        this.emitter = new EventEmitter();
    }

    process(data: string): string {
        const result = transform(data);
        this.emitter.emit('done', result);
        return result;
    }
}

export function transform(input: string): string {
    return input.trim().toUpperCase();
}

export function buildPipeline(): DataPipeline {
    const p = new DataPipeline();
    return p;
}
