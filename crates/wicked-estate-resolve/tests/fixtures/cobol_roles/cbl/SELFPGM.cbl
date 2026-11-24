      * A program that CALLs itself from a paragraph: a recursive call.
       IDENTIFICATION DIVISION.
       PROGRAM-ID. SELFPGM.
       PROCEDURE DIVISION.
       MAIN-PARA.
           CALL 'SELFPGM'
           GOBACK.
