import { Component } from '@angular/core';
import { Child } from './child';

@Component({ selector: 'app-ext', standalone: true, imports: [Child], templateUrl: './ext.component.html' })
export class Ext {
  title = 't';
}
