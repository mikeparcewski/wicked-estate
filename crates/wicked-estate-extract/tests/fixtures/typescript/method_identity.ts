import { other } from "./other";
export class Repo { save(): void {} update(): void { this.save(); other.save(); } }
export class Cache {
  save(): void {}
  flush(): void { this.save(); const cb = () => this.save(); const save = () => {}; save(); }
}
export interface Store { save(): void; }
export const lit = { save() {}, run() { this.save(); } };
export function top() { const r = new Repo(); r.update(); }
