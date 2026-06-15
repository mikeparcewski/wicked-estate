import { EventEmitter } from 'events';
import { createHash } from 'crypto';

export interface Serializer<T> {
    serialize(value: T): string;
    deserialize(raw: string): T;
}

export enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

export type HandlerId = string;
export type Handler<T> = (event: T) => void;

export const MAX_QUEUE_SIZE: number = 1000;
export const DEFAULT_LOG_LEVEL: LogLevel = LogLevel.Info;

export const createId = (): HandlerId => createHash('sha256').digest('hex').slice(0, 8);

let queueDepth = 0;

export class MessageQueue<T> implements Serializer<T> {
    private emitter: EventEmitter;
    private readonly id: HandlerId;

    constructor() {
        this.id = createId();
        this.emitter = new EventEmitter();
    }

    enqueue(item: T): void {
        queueDepth++;
        this.emitter.emit('item', item);
    }

    serialize(value: T): string {
        return JSON.stringify(value);
    }

    deserialize(raw: string): T {
        return JSON.parse(raw) as T;
    }
}

export function drainQueue<T>(queue: MessageQueue<T>, items: T[]): void {
    items.forEach(item => queue.enqueue(item));
}

export function buildQueue<T>(): MessageQueue<T> {
    return new MessageQueue<T>();
}
