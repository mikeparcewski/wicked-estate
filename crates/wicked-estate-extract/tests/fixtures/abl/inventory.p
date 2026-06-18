/* Progress OpenEdge ABL procedure module — inventory adjustment with an internal
   function and a couple of procedures that call each other. */
DEFINE VARIABLE gWarehouse AS CHARACTER NO-UNDO.

FUNCTION onHandQty RETURNS INTEGER (INPUT pSku AS CHARACTER):
  DEFINE VARIABLE qty AS INTEGER NO-UNDO.
  qty = lookupStock(pSku).
  RETURN qty.
END FUNCTION.

PROCEDURE adjustStock:
  DEFINE INPUT PARAMETER pSku AS CHARACTER NO-UNDO.
  DEFINE INPUT PARAMETER pDelta AS INTEGER NO-UNDO.
  DEFINE VARIABLE current AS INTEGER NO-UNDO.
  current = onHandQty(pSku).
  RUN writeLedger.
END PROCEDURE.

PROCEDURE writeLedger:
  MESSAGE "ledger updated".
END PROCEDURE.

RUN adjustStock.
