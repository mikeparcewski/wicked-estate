import { Component, Directive, EventEmitter, Input, Output, output, signal } from '@angular/core';
import { Child } from './child';

// TS-S4 cases: aliased, signal and inherited outputs; two directives declaring one output name;
// DOM vs directive events (including an output named like a DOM event); handler calls with
// `$event` whole and derived; assignments; chained handlers; same-name handlers in two hosts.
@Directive()
export abstract class EvBase {
  @Output() baseEv = new EventEmitter<string>();
}

@Component({ selector: 'ev-child', standalone: true, template: `` })
export class EvChild extends EvBase {
  @Input() label = '';
  @Output() picked = new EventEmitter<string>();
  @Output('renamedOut') changed = new EventEmitter<number>();
  closed = output<void>();
}

@Directive({ selector: '[pingA]', standalone: true })
export class PingA {
  @Output() ping = new EventEmitter<number>();
}

@Directive({ selector: '[pingB]', standalone: true })
export class PingB {
  @Output() ping = new EventEmitter<number>();
}

@Directive({ selector: '[clicky]', standalone: true })
export class Clicky {
  @Output() click = new EventEmitter<string>();
}

@Component({
  selector: 'ev-host',
  standalone: true,
  imports: [EvChild, PingA, PingB, Clicky],
  template: `
    <ev-child (picked)="onPick($event, n)" (renamedOut)="n = $event" (closed)="done(); n = 2" (baseEv)="onPick($event.trim(), n)"></ev-child>
    <div pingA pingB (ping)="pinged($event)"></div>
    <button (click)="done()">dom</button>
    <span clicky (click)="onPick($event, n)">out</span>
    <!-- 🙂 --><ev-child (picked)="last = $event"></ev-child>
  `,
})
export class EvHost {
  n = 0;
  last = '';
  onPick(value: string, count: number): void {
    const seen = value;
  }
  pinged(p: number): void {}
  done(): void {}
}

@Component({
  selector: 'ev-host2',
  standalone: true,
  imports: [EvChild],
  template: `<ev-child (picked)="onPick($event)"></ev-child>`,
})
export class EvHost2 {
  onPick(other: string): void {}
}

// codex S4 r1 cases: a template-local `@let` handler shadowing a host method, and a handler with an
// explicit TypeScript `this` parameter.
@Component({
  selector: 'ev-host3',
  standalone: true,
  imports: [EvChild, Child],
  template: `@let pick = other; <ev-child (picked)="pick($event)"></ev-child><ev-child (picked)="withThis($event, 1)"></ev-child>@let mine = sig; <app-child [(value)]="mine"></app-child>`,
})
export class EvHost3 {
  other = (x: string): void => {};
  sig = signal(0);
  mine = 0; // shadowed in the template by `@let mine`
  pick(v: string): void {}
  withThis(this: EvHost3, value: string, n: number): void {}
}
