      * The COACTUPC / CSUTLDPY shape: paragraphs PERFORMed by name from a procedure copybook
      * (not nodes), and a COPYd data item of the same spelling.
       IDENTIFICATION DIVISION.
       PROGRAM-ID. DATEPGM.
       DATA DIVISION.
       WORKING-STORAGE SECTION.
       COPY DATEREC.
       PROCEDURE DIVISION.
       MAIN-PARA.
           PERFORM EDIT-DATE THRU EDIT-DATE-EXIT
           GOBACK.
       COPY DATEPROC.
