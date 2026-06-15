// sample.swift — payment domain model with protocol, struct, class, and extension

import Foundation

// MARK: - Protocols

/// Identifies any domain entity that has a stable string identifier.
protocol Identifiable {
    var id: String { get }
}

/// A type that can be persisted to and restored from a dictionary.
protocol Persistable {
    func toDictionary() -> [String: Any]
    static func from(dictionary: [String: Any]) throws -> Self
}

/// Validates its own state and throws a domain error if invalid.
protocol Validatable {
    func validate() throws
}

// MARK: - Enums

enum Currency: String, Codable, CaseIterable {
    case usd = "USD"
    case eur = "EUR"
    case gbp = "GBP"
    case jpy = "JPY"
}

enum PaymentStatus: String, Codable {
    case pending   = "PENDING"
    case succeeded = "SUCCEEDED"
    case failed    = "FAILED"
    case refunded  = "REFUNDED"
}

// MARK: - Errors

enum PaymentError: LocalizedError {
    case notFound(id: String)
    case insufficientFunds(available: Int, required: Int)
    case invalidAmount(reason: String)
    case networkFailure(underlying: Error)

    var errorDescription: String? {
        switch self {
        case .notFound(let id):
            return "Payment '\(id)' not found."
        case .insufficientFunds(let available, let required):
            return "Insufficient funds: have \(available) cents, need \(required) cents."
        case .invalidAmount(let reason):
            return "Invalid amount: \(reason)"
        case .networkFailure(let error):
            return "Network failure: \(error.localizedDescription)"
        }
    }
}

// MARK: - Value types (Structs)

/// Represents a monetary amount in the smallest currency unit (e.g. cents).
struct Money: Codable, Equatable, CustomStringConvertible {
    let amountCents: Int
    let currency: Currency

    var description: String {
        let major = Double(amountCents) / 100.0
        return String(format: "%.2f %@", major, currency.rawValue)
    }

    static func zero(_ currency: Currency) -> Money {
        Money(amountCents: 0, currency: currency)
    }

    func adding(_ other: Money) throws -> Money {
        guard currency == other.currency else {
            throw PaymentError.invalidAmount(reason: "Currency mismatch: \(currency) vs \(other.currency)")
        }
        return Money(amountCents: amountCents + other.amountCents, currency: currency)
    }

    func subtracting(_ other: Money) throws -> Money {
        guard currency == other.currency else {
            throw PaymentError.invalidAmount(reason: "Currency mismatch: \(currency) vs \(other.currency)")
        }
        guard amountCents >= other.amountCents else {
            throw PaymentError.insufficientFunds(available: amountCents, required: other.amountCents)
        }
        return Money(amountCents: amountCents - other.amountCents, currency: currency)
    }
}

/// A snapshot of a payment transaction.
struct Payment: Identifiable, Codable, Validatable {
    let id: String
    let userId: String
    let amount: Money
    var status: PaymentStatus
    let createdAt: Date
    var updatedAt: Date

    func validate() throws {
        guard !id.isEmpty else {
            throw PaymentError.invalidAmount(reason: "Payment id must not be empty.")
        }
        guard !userId.isEmpty else {
            throw PaymentError.invalidAmount(reason: "User id must not be empty.")
        }
        guard amount.amountCents > 0 else {
            throw PaymentError.invalidAmount(reason: "Amount must be positive.")
        }
    }
}

// MARK: - Reference types (Class)

/// In-memory payment ledger. Not thread-safe on its own — wrap in an actor for concurrent use.
final class PaymentLedger {
    private var payments: [String: Payment] = [:]

    var count: Int { payments.count }

    func insert(_ payment: Payment) throws {
        try payment.validate()
        payments[payment.id] = payment
    }

    func find(id: String) throws -> Payment {
        guard let payment = payments[id] else {
            throw PaymentError.notFound(id: id)
        }
        return payment
    }

    func updateStatus(id: String, to status: PaymentStatus) throws {
        guard var payment = payments[id] else {
            throw PaymentError.notFound(id: id)
        }
        payment.status = status
        payment.updatedAt = Date()
        payments[id] = payment
    }

    func all(for userId: String) -> [Payment] {
        payments.values.filter { $0.userId == userId }
                       .sorted { $0.createdAt < $1.createdAt }
    }

    func total(for userId: String) throws -> Money {
        let userPayments = all(for: userId).filter { $0.status == .succeeded }
        guard let first = userPayments.first else {
            return Money(amountCents: 0, currency: .usd)
        }
        return try userPayments.dropFirst().reduce(first.amount) { acc, p in
            try acc.adding(p.amount)
        }
    }
}

// MARK: - Extensions

extension Payment: CustomStringConvertible {
    var description: String {
        "Payment(id: \(id), amount: \(amount), status: \(status.rawValue))"
    }
}

extension Money: Comparable {
    static func < (lhs: Money, rhs: Money) -> Bool {
        lhs.amountCents < rhs.amountCents
    }
}

extension PaymentLedger: CustomStringConvertible {
    var description: String {
        "PaymentLedger(\(count) payment\(count == 1 ? "" : "s"))"
    }
}
