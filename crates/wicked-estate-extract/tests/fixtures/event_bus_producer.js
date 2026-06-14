// Fixture: event-bus producer
// Emits two distinct topics. Used by tests/extra_edge.rs.

function placeOrder(order) {
    validateOrder(order);
    db.save(order);
    bus.emit("orders.created", { orderId: order.id });
}

function fulfillOrder(orderId) {
    const order = db.load(orderId);
    ship(order);
    bus.emit("orders.fulfilled", { orderId });
}

function cancelOrder(orderId) {
    db.cancel(orderId);
    bus.emit("orders.created", { orderId, status: "cancelled" }); // reuses orders.created intentionally
}
