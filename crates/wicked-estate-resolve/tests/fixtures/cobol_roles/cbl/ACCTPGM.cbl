       IDENTIFICATION DIVISION.
       PROGRAM-ID. ACCTPGM.
       DATA DIVISION.
       WORKING-STORAGE SECTION.
      * Another program's own storage: never FRAGPGM's.
       01 ACCT.
          05 BAL PIC 9(4).
       PROCEDURE DIVISION.
       MAIN-PARA.
           GOBACK.
