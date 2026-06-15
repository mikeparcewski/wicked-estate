import { readFileSync } from 'fs';

const MAX_CHUNK_SIZE = 4096;
const DEFAULT_ENCODING = 'utf-8';

const createFormatter = (prefix) => (msg) => `[${prefix}] ${msg}`;

let requestCount = 0;

class HttpClient {
    constructor(baseUrl) {
        this.baseUrl = baseUrl;
        this.headers = {};
    }

    setHeader(key, value) {
        this.headers[key] = value;
    }

    get(path) {
        requestCount++;
        return fetch(this.baseUrl + path, { headers: this.headers });
    }
}

export function buildClient(baseUrl) {
    const client = new HttpClient(baseUrl);
    client.setHeader('Content-Type', 'application/json');
    return client;
}

export function fetchData(url, path) {
    const client = buildClient(url);
    return client.get(path);
}

export function parseResponse(raw) {
    return JSON.parse(raw);
}
