import { Input } from '@angular/core';
import { CustomerService } from './customer.service';

export class CustomerComponent {
    @Input() tenantId!: string;
    customerId = '';

    load(route: any, service: CustomerService) {
        const routeId = route.snapshot.paramMap.get('id');
        this.customerId = routeId;
        return this.loadCustomer(this.customerId, service);
    }

    loadCustomer(id: string, service: CustomerService): string {
        const customer = service.getCustomer(id);
        return customer;
    }
}
