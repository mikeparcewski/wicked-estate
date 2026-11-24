import { helper } from '@lib/util';
export class UserService {
  get(id: string): string { return helper(id).trim(); }
  get2(id: number): string { return this.get(String(id)); }
}
export function top(x: string) { const s = new UserService(); return s.get(x); }
