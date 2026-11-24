      * A label two sections define, performed from a third: the extractor cannot pick one
      * header, so it stays a label ref by name. A data item has the same spelling.
       IDENTIFICATION DIVISION.
       PROGRAM-ID. AMBPGM.
       DATA DIVISION.
       WORKING-STORAGE SECTION.
       01 STEP-PARA PIC X.
       PROCEDURE DIVISION.
       MAIN-SECT SECTION.
       MAIN-PARA.
           PERFORM STEP-PARA
           GOBACK.
       FIRST-SECT SECTION.
       STEP-PARA.
           EXIT.
       SECOND-SECT SECTION.
       STEP-PARA.
           EXIT.
