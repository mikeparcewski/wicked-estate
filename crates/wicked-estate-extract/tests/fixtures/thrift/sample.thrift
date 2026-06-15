// sample.thrift — payment processing IDL

namespace java   com.example.payments
namespace go     payments
namespace py     payments

// ── Enums ────────────────────────────────────────────────────────────────────

enum PaymentStatus {
  PENDING   = 1,
  SUCCEEDED = 2,
  FAILED    = 3,
  REFUNDED  = 4
}

enum Currency {
  USD = 1,
  EUR = 2,
  GBP = 3,
  JPY = 4
}

// ── Exceptions ───────────────────────────────────────────────────────────────

exception PaymentNotFoundException {
  1: required string payment_id,
  2: required string message
}

exception InsufficientFundsException {
  1: required i64    available_cents,
  2: required i64    required_cents,
  3: optional string message
}

exception ValidationException {
  1: required string field,
  2: required string reason
}

// ── Structs ──────────────────────────────────────────────────────────────────

struct Money {
  1: required i64      amount_cents,
  2: required Currency currency
}

struct PaymentMethod {
  1: required string id,
  2: required string type,          // "card", "bank_transfer", "wallet"
  3: optional string last_four,
  4: optional string bank_code
}

struct Payment {
  1: required string        id,
  2: required string        user_id,
  3: required Money         amount,
  4: required PaymentStatus status,
  5: required PaymentMethod method,
  6: required i64           created_at,   // Unix timestamp ms
  7: optional i64           settled_at,
  8: optional string        failure_reason
}

struct CreatePaymentRequest {
  1: required string        user_id,
  2: required Money         amount,
  3: required PaymentMethod method,
  4: optional string        idempotency_key
}

struct RefundRequest {
  1: required string payment_id,
  2: optional Money  amount,         // partial refund if set; full if absent
  3: optional string reason
}

struct ListPaymentsRequest {
  1: required string        user_id,
  2: optional PaymentStatus status,
  3: optional i32           limit    = 50,
  4: optional string        cursor
}

struct ListPaymentsResponse {
  1: required list<Payment> payments,
  2: optional string        next_cursor
}

// ── Service ──────────────────────────────────────────────────────────────────

service PaymentService {

  // CreatePayment initiates a new payment transaction.
  Payment createPayment(
    1: CreatePaymentRequest request
  ) throws (
    1: InsufficientFundsException insufficient_funds,
    2: ValidationException         validation_error
  ),

  // GetPayment retrieves a payment by its ID.
  Payment getPayment(
    1: string payment_id
  ) throws (
    1: PaymentNotFoundException not_found
  ),

  // ListPayments returns a paginated list of payments for a user.
  ListPaymentsResponse listPayments(
    1: ListPaymentsRequest request
  ),

  // RefundPayment issues a full or partial refund.
  Payment refundPayment(
    1: RefundRequest request
  ) throws (
    1: PaymentNotFoundException not_found,
    2: ValidationException       validation_error
  ),

  // HealthCheck returns true when the service is operational.
  bool healthCheck()
}
