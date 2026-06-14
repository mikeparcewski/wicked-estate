import { readFile } from 'fs';

const MAX_LISTENERS = 10;
const DEFAULT_DELAY = 100;

const createHandler = (fn) => (data) => fn(data);

let instanceCount = 0;

class EventBus {
    constructor() {
        this.listeners = {};
    }

    on(event, handler) {
        if (!this.listeners[event]) {
            this.listeners[event] = [];
        }
        this.listeners[event].push(handler);
    }

    emit(event, data) {
        const handlers = this.listeners[event] || [];
        handlers.forEach(h => h(data));
    }
}

function createBus() {
    instanceCount++;
    return new EventBus();
}

function processData(data) {
    const bus = createBus();
    bus.emit('data', data);
    return serialize(data);
}

function serialize(data) {
    return JSON.stringify(data);
}
