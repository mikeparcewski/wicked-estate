public with sharing class AccountService {

    public static Account getById(Id accountId) {
        return [SELECT Id, Name, Type, AnnualRevenue FROM Account WHERE Id = :accountId LIMIT 1];
    }

    public static List<Account> getByType(String accountType) {
        return [SELECT Id, Name, AnnualRevenue FROM Account WHERE Type = :accountType ORDER BY Name];
    }

    public static Account create(String name, String type) {
        Account acc = new Account(Name = name, Type = type);
        insert acc;
        return acc;
    }

    public static void updateRevenue(Id accountId, Decimal revenue) {
        Account acc = new Account(Id = accountId, AnnualRevenue = revenue);
        update acc;
    }

    public static void deactivate(Id accountId) {
        Account acc = new Account(Id = accountId, Active__c = false);
        update acc;
    }
}
