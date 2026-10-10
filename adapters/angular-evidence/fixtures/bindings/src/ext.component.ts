import { Component } from '@angular/core';
import { Child } from './child';
import { EvChild } from './events';

@Component({ selector: 'app-ext', standalone: true, imports: [Child, EvChild], templateUrl: './ext.component.html' })
export class Ext {
  title = 't';
}
