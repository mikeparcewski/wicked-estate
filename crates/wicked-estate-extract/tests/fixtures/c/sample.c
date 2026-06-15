#include <stdio.h>
#include <string.h>
#include <stdlib.h>

typedef struct {
    char name[64];
    double balance;
    int account_id;
} Account;

Account create_account(int id, const char *name, double initial_balance) {
    Account acc;
    acc.account_id = id;
    strncpy(acc.name, name, sizeof(acc.name) - 1);
    acc.name[sizeof(acc.name) - 1] = '\0';
    acc.balance = initial_balance;
    return acc;
}

int deposit(Account *acc, double amount) {
    if (amount <= 0.0) {
        fprintf(stderr, "deposit: amount must be positive\n");
        return -1;
    }
    acc->balance += amount;
    return 0;
}

int withdraw(Account *acc, double amount) {
    if (amount <= 0.0) {
        fprintf(stderr, "withdraw: amount must be positive\n");
        return -1;
    }
    if (acc->balance < amount) {
        fprintf(stderr, "withdraw: insufficient funds\n");
        return -2;
    }
    acc->balance -= amount;
    return 0;
}

void print_account(const Account *acc) {
    printf("Account[%d] %-20s balance: %.2f\n",
           acc->account_id, acc->name, acc->balance);
}

int main(void) {
    Account a = create_account(1001, "Alice", 500.0);
    deposit(&a, 250.0);
    withdraw(&a, 100.0);
    print_account(&a);
    return 0;
}
