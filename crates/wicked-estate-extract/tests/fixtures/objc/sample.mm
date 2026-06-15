// sample.mm — Objective-C++ payment model
// Uses ObjC classes + a C++ helper struct to exercise .mm mixed-mode parsing.

#import <Foundation/Foundation.h>
#include <cmath>
#include <string>

// ── C++ helper (mixed-mode .mm) ───────────────────────────────────────────

struct MoneyValue {
    long long amountCents;
    std::string currency;  // ISO 4217

    double majorAmount() const {
        return static_cast<double>(amountCents) / 100.0;
    }

    MoneyValue adding(const MoneyValue& other) const {
        return MoneyValue{amountCents + other.amountCents, currency};
    }
};

// ── Enums ─────────────────────────────────────────────────────────────────

typedef NS_ENUM(NSInteger, WEPaymentStatus) {
    WEPaymentStatusPending   = 0,
    WEPaymentStatusSucceeded = 1,
    WEPaymentStatusFailed    = 2,
    WEPaymentStatusRefunded  = 3,
};

typedef NS_ENUM(NSInteger, WECurrency) {
    WECurrencyUSD = 0,
    WECurrencyEUR = 1,
    WECurrencyGBP = 2,
};

// ── Error domain ──────────────────────────────────────────────────────────

extern NSErrorDomain const WEPaymentErrorDomain;
NSErrorDomain const WEPaymentErrorDomain = @"com.example.wicked-estate.payment";

typedef NS_ERROR_ENUM(WEPaymentErrorDomain, WEPaymentErrorCode) {
    WEPaymentErrorCodeNotFound         = 1001,
    WEPaymentErrorCodeInsufficientFunds = 1002,
    WEPaymentErrorCodeInvalidAmount    = 1003,
};

// ── WEMoney ───────────────────────────────────────────────────────────────

@interface WEMoney : NSObject <NSCopying>

@property (nonatomic, readonly) long long amountCents;
@property (nonatomic, readonly) WECurrency currency;
@property (nonatomic, readonly) double majorAmount;

+ (instancetype)moneyWithAmountCents:(long long)cents currency:(WECurrency)currency;
- (instancetype)initWithAmountCents:(long long)cents currency:(WECurrency)currency NS_DESIGNATED_INITIALIZER;
- (instancetype)init NS_UNAVAILABLE;

- (nullable WEMoney *)addingMoney:(WEMoney *)other error:(NSError **)error;
- (NSString *)formattedString;

@end

@implementation WEMoney

+ (instancetype)moneyWithAmountCents:(long long)cents currency:(WECurrency)currency {
    return [[self alloc] initWithAmountCents:cents currency:currency];
}

- (instancetype)initWithAmountCents:(long long)cents currency:(WECurrency)currency {
    self = [super init];
    if (self) {
        _amountCents = cents;
        _currency    = currency;
    }
    return self;
}

- (double)majorAmount {
    return static_cast<double>(_amountCents) / 100.0;
}

- (nullable WEMoney *)addingMoney:(WEMoney *)other error:(NSError **)error {
    if (other.currency != _currency) {
        if (error) {
            *error = [NSError errorWithDomain:WEPaymentErrorDomain
                                         code:WEPaymentErrorCodeInvalidAmount
                                     userInfo:@{
                NSLocalizedDescriptionKey: @"Cannot add amounts with different currencies."
            }];
        }
        return nil;
    }
    return [[WEMoney alloc] initWithAmountCents:_amountCents + other.amountCents
                                       currency:_currency];
}

- (NSString *)formattedString {
    static NSString * const symbols[] = {@"USD", @"EUR", @"GBP"};
    NSString *symbol = (_currency < 3) ? symbols[_currency] : @"???";
    return [NSString stringWithFormat:@"%.2f %@", self.majorAmount, symbol];
}

- (id)copyWithZone:(NSZone *)zone {
    return [[WEMoney allocWithZone:zone] initWithAmountCents:_amountCents currency:_currency];
}

- (NSString *)description {
    return [NSString stringWithFormat:@"<WEMoney: %@>", [self formattedString]];
}

@end

// ── WEPayment ─────────────────────────────────────────────────────────────

@interface WEPayment : NSObject

@property (nonatomic, copy,   readonly)  NSString        *paymentId;
@property (nonatomic, copy,   readonly)  NSString        *userId;
@property (nonatomic, strong, readonly)  WEMoney         *amount;
@property (nonatomic, assign, readwrite) WEPaymentStatus  status;
@property (nonatomic, strong, readonly)  NSDate          *createdAt;
@property (nonatomic, strong, readwrite) NSDate          *updatedAt;

+ (instancetype)paymentWithId:(NSString *)paymentId
                       userId:(NSString *)userId
                       amount:(WEMoney *)amount;

- (instancetype)initWithId:(NSString *)paymentId
                    userId:(NSString *)userId
                    amount:(WEMoney *)amount NS_DESIGNATED_INITIALIZER;
- (instancetype)init NS_UNAVAILABLE;

- (BOOL)transitionToStatus:(WEPaymentStatus)newStatus error:(NSError **)error;
- (NSDictionary<NSString *, id> *)dictionaryRepresentation;

@end

@implementation WEPayment

+ (instancetype)paymentWithId:(NSString *)paymentId
                       userId:(NSString *)userId
                       amount:(WEMoney *)amount {
    return [[self alloc] initWithId:paymentId userId:userId amount:amount];
}

- (instancetype)initWithId:(NSString *)paymentId
                    userId:(NSString *)userId
                    amount:(WEMoney *)amount {
    NSParameterAssert(paymentId.length > 0);
    NSParameterAssert(userId.length > 0);
    NSParameterAssert(amount != nil);

    self = [super init];
    if (self) {
        _paymentId = [paymentId copy];
        _userId    = [userId copy];
        _amount    = amount;
        _status    = WEPaymentStatusPending;
        _createdAt = [NSDate date];
        _updatedAt = _createdAt;
    }
    return self;
}

- (BOOL)transitionToStatus:(WEPaymentStatus)newStatus error:(NSError **)error {
    // Refunded and Failed are terminal states.
    if (_status == WEPaymentStatusRefunded || _status == WEPaymentStatusFailed) {
        if (error) {
            *error = [NSError errorWithDomain:WEPaymentErrorDomain
                                         code:WEPaymentErrorCodeInvalidAmount
                                     userInfo:@{
                NSLocalizedDescriptionKey:
                    [NSString stringWithFormat:
                     @"Cannot transition from terminal status %ld.", (long)_status]
            }];
        }
        return NO;
    }
    _status    = newStatus;
    _updatedAt = [NSDate date];
    return YES;
}

- (NSDictionary<NSString *, id> *)dictionaryRepresentation {
    return @{
        @"id":         _paymentId,
        @"user_id":    _userId,
        @"amount":     @(_amount.amountCents),
        @"status":     @(_status),
        @"created_at": [_createdAt description],
        @"updated_at": [_updatedAt description],
    };
}

- (NSString *)description {
    return [NSString stringWithFormat:
            @"<WEPayment id=%@ user=%@ amount=%@ status=%ld>",
            _paymentId, _userId, [_amount formattedString], (long)_status];
}

@end

// ── WEPaymentLedger ───────────────────────────────────────────────────────

@interface WEPaymentLedger : NSObject

- (void)addPayment:(WEPayment *)payment;
- (nullable WEPayment *)paymentWithId:(NSString *)paymentId error:(NSError **)error;
- (NSArray<WEPayment *> *)paymentsForUserId:(NSString *)userId;
- (NSUInteger)count;

@end

@implementation WEPaymentLedger {
    NSMutableDictionary<NSString *, WEPayment *> *_store;
}

- (instancetype)init {
    self = [super init];
    if (self) {
        _store = [NSMutableDictionary dictionary];
    }
    return self;
}

- (void)addPayment:(WEPayment *)payment {
    NSParameterAssert(payment != nil);
    _store[payment.paymentId] = payment;
}

- (nullable WEPayment *)paymentWithId:(NSString *)paymentId error:(NSError **)error {
    WEPayment *payment = _store[paymentId];
    if (!payment && error) {
        *error = [NSError errorWithDomain:WEPaymentErrorDomain
                                     code:WEPaymentErrorCodeNotFound
                                 userInfo:@{
            NSLocalizedDescriptionKey:
                [NSString stringWithFormat:@"Payment '%@' not found.", paymentId]
        }];
    }
    return payment;
}

- (NSArray<WEPayment *> *)paymentsForUserId:(NSString *)userId {
    NSPredicate *pred = [NSPredicate predicateWithFormat:@"userId == %@", userId];
    return [[_store allValues] filteredArrayUsingPredicate:pred];
}

- (NSUInteger)count {
    return _store.count;
}

@end
