using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;

namespace Example.Services
{
    public class OrderService
    {
        private readonly IOrderRepository _repository;

        public int RetryLimit { get; set; }
        public bool HasRepository => _repository != null;

        public OrderService(IOrderRepository repository)
        {
            _repository = repository;
        }

        public async Task<Order> GetByIdAsync(Guid id)
        {
            return await _repository.FindByIdAsync(id);
        }

        public async Task<IEnumerable<Order>> GetPendingAsync()
        {
            var orders = await _repository.GetAllAsync();
            return orders.Where(o => o.Status == OrderStatus.Pending);
        }

        public async Task<Order> PlaceOrderAsync(Cart cart)
        {
            var order = Order.FromCart(cart);
            await _repository.SaveAsync(order);
            return order;
        }

        public async Task CancelAsync(Guid id)
        {
            var order = await _repository.FindByIdAsync(id);
            order?.Cancel();
            if (order != null)
                await _repository.SaveAsync(order);
        }
    }
}
