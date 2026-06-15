// sample.dart — order management domain with abstract class, concrete class,
// factory constructor, and service layer.

import 'dart:async';

// ── Enums ─────────────────────────────────────────────────────────────────

enum OrderStatus {
  pending,
  confirmed,
  shipped,
  delivered,
  cancelled;

  bool get isFinal => this == delivered || this == cancelled;

  String get displayName => name[0].toUpperCase() + name.substring(1);
}

enum Currency { usd, eur, gbp }

// ── Exceptions ────────────────────────────────────────────────────────────

class OrderException implements Exception {
  final String message;
  const OrderException(this.message);

  @override
  String toString() => 'OrderException: $message';
}

class OrderNotFoundException extends OrderException {
  final String orderId;
  const OrderNotFoundException(this.orderId)
      : super('Order "$orderId" not found.');
}

// ── Value objects ─────────────────────────────────────────────────────────

class Money {
  final int amountCents;
  final Currency currency;

  const Money(this.amountCents, this.currency);

  factory Money.zero([Currency currency = Currency.usd]) =>
      Money(0, currency);

  Money operator +(Money other) {
    _assertSameCurrency(other);
    return Money(amountCents + other.amountCents, currency);
  }

  Money operator -(Money other) {
    _assertSameCurrency(other);
    if (amountCents < other.amountCents) {
      throw OrderException('Subtraction would yield negative amount.');
    }
    return Money(amountCents - other.amountCents, currency);
  }

  bool operator >(Money other) {
    _assertSameCurrency(other);
    return amountCents > other.amountCents;
  }

  void _assertSameCurrency(Money other) {
    if (currency != other.currency) {
      throw OrderException(
          'Currency mismatch: ${currency.name} vs ${other.currency.name}');
    }
  }

  @override
  String toString() {
    final major = amountCents / 100;
    return '${major.toStringAsFixed(2)} ${currency.name.toUpperCase()}';
  }
}

// ── Abstract base ─────────────────────────────────────────────────────────

abstract class Entity {
  final String id;
  final DateTime createdAt;

  const Entity({required this.id, required this.createdAt});

  @override
  bool operator ==(Object other) =>
      identical(this, other) || (other is Entity && other.id == id);

  @override
  int get hashCode => id.hashCode;
}

// ── Concrete domain types ──────────────────────────────────────────────────

class OrderItem {
  final String productSku;
  final int quantity;
  final Money unitPrice;

  const OrderItem({
    required this.productSku,
    required this.quantity,
    required this.unitPrice,
  });

  Money get lineTotal => Money(unitPrice.amountCents * quantity, unitPrice.currency);

  @override
  String toString() => 'OrderItem($productSku x$quantity @ $unitPrice)';
}

class Order extends Entity {
  final String userId;
  final List<OrderItem> items;
  OrderStatus status;
  DateTime updatedAt;

  Order({
    required super.id,
    required super.createdAt,
    required this.userId,
    required this.items,
    this.status = OrderStatus.pending,
    DateTime? updatedAt,
  }) : updatedAt = updatedAt ?? createdAt;

  /// Factory constructor: create a brand-new order from raw inputs.
  factory Order.create({
    required String id,
    required String userId,
    required List<OrderItem> items,
  }) {
    if (items.isEmpty) {
      throw OrderException('Cannot create an order with no items.');
    }
    return Order(
      id: id,
      createdAt: DateTime.now().toUtc(),
      userId: userId,
      items: List.unmodifiable(items),
    );
  }

  /// Factory constructor: deserialise from a JSON map.
  factory Order.fromJson(Map<String, dynamic> json) {
    return Order(
      id: json['id'] as String,
      createdAt: DateTime.parse(json['created_at'] as String),
      updatedAt: DateTime.parse(json['updated_at'] as String),
      userId: json['user_id'] as String,
      items: (json['items'] as List<dynamic>).map((e) {
        final m = e as Map<String, dynamic>;
        return OrderItem(
          productSku: m['sku'] as String,
          quantity: m['quantity'] as int,
          unitPrice: Money(m['unit_cents'] as int, Currency.usd),
        );
      }).toList(),
      status: OrderStatus.values.byName(json['status'] as String),
    );
  }

  Money get total => items.fold(
        Money.zero(),
        (acc, item) => acc + item.lineTotal,
      );

  void transitionTo(OrderStatus next) {
    if (status.isFinal) {
      throw OrderException(
          'Cannot transition from final status "${status.displayName}".');
    }
    status = next;
    updatedAt = DateTime.now().toUtc();
  }

  Map<String, dynamic> toJson() => {
        'id': id,
        'user_id': userId,
        'status': status.name,
        'total_cents': total.amountCents,
        'created_at': createdAt.toIso8601String(),
        'updated_at': updatedAt.toIso8601String(),
      };

  @override
  String toString() =>
      'Order($id, $userId, ${status.displayName}, total: $total)';
}

// ── Service layer ──────────────────────────────────────────────────────────

class OrderRepository {
  final Map<String, Order> _store = {};

  Future<void> save(Order order) async {
    _store[order.id] = order;
  }

  Future<Order> findById(String id) async {
    final order = _store[id];
    if (order == null) throw OrderNotFoundException(id);
    return order;
  }

  Future<List<Order>> findByUserId(String userId) async {
    return _store.values
        .where((o) => o.userId == userId)
        .toList()
      ..sort((a, b) => b.createdAt.compareTo(a.createdAt));
  }

  Future<void> delete(String id) async {
    if (_store.remove(id) == null) throw OrderNotFoundException(id);
  }
}

class OrderService {
  final OrderRepository _repo;

  OrderService(this._repo);

  Future<Order> placeOrder({
    required String orderId,
    required String userId,
    required List<OrderItem> items,
  }) async {
    final order = Order.create(id: orderId, userId: userId, items: items);
    await _repo.save(order);
    return order;
  }

  Future<Order> confirmOrder(String orderId) async {
    final order = await _repo.findById(orderId);
    order.transitionTo(OrderStatus.confirmed);
    await _repo.save(order);
    return order;
  }

  Future<Order> cancelOrder(String orderId) async {
    final order = await _repo.findById(orderId);
    order.transitionTo(OrderStatus.cancelled);
    await _repo.save(order);
    return order;
  }

  Future<List<Order>> getOrdersForUser(String userId) =>
      _repo.findByUserId(userId);
}
