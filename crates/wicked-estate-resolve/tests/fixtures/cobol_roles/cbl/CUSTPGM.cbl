       IDENTIFICATION DIVISION.
       PROGRAM-ID. CUSTPGM.
       DATA DIVISION.
       WORKING-STORAGE SECTION.
       COPY CUSTREC.
       COPY ORDREC.
       01 CALC PIC 9(4).
       PROCEDURE DIVISION.
       MAIN-PARA.
           MOVE 1 TO ID OF CUSTOMER.
           MOVE 2 TO ID IN ORDER-REC.
           MOVE 3 TO CALC.
           MOVE SPACES TO CUST-NAME.
           MOVE 1 TO WS-DONE.
           PERFORM CALC.
           PERFORM CHECK-PARA THRU CHECK-EXIT.
           CALL 'SHIPPGM'.
           EXEC CICS LINK PROGRAM('SHIPPGM') COMMAREA(CUSTOMER)
           END-EXEC.
           GO TO CHECK-EXIT.
       CALC.
           EXIT.
       CHECK-PARA.
           EXIT.
       CHECK-EXIT.
           GOBACK.
       SHIPPGM.
           EXIT.
