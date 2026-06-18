/* Procedure-style ABL (.p) — FUNCTION with RETURNS, internal PROCEDURE, RUN, and calls. */
DEFINE VARIABLE gTotal AS DECIMAL NO-UNDO.

FUNCTION calcLineTotal RETURNS DECIMAL (INPUT qty AS INTEGER, INPUT price AS DECIMAL):
  RETURN qty * price.
END FUNCTION.

PROCEDURE processOrder:
  DEFINE INPUT PARAMETER pOrderId AS INTEGER NO-UNDO.
  gTotal = calcLineTotal(10, 2.50).
  RUN postLedger.
END PROCEDURE.

PROCEDURE postLedger:
  MESSAGE "posted".
END PROCEDURE.

RUN processOrder.
