// Fixture: event-bus consumer
// Subscribes to the same topics the producer emits. Used by tests/extra_edge.rs.

bus.subscribe("orders.created", async (event) => {
    await notifyWarehouse(event.orderId);
});

bus.subscribe("orders.fulfilled", async (event) => {
    await sendShippingConfirmation(event.orderId);
});

// A consumer that also subscribes to an additional topic not in the producer.
bus.subscribe("payments.processed", async (event) => {
    await recordRevenue(event.amount);
});
