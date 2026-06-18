/* Progress OpenEdge ABL class — exercises CLASS, VAR, METHOD (with access + return type),
   CONSTRUCTOR, function calls, RUN, and a nested method body. */
CLASS FunkyWorld.Util.KittyLogHelper:
  VAR PRIVATE INT64 MAX_RECORDS = 5.
  VAR PUBLIC CHARACTER SomeName.

  CONSTRUCTOR PUBLIC KittyLogHelper():
    SomeName = "default".
  END CONSTRUCTOR.

  METHOD PUBLIC VOID setup():
    oObject = NEW Object().
    RUN initialize.
  END METHOD.

  METHOD PUBLIC INTEGER computeTotal(INPUT pItems AS INTEGER):
    DEFINE VARIABLE total AS INTEGER NO-UNDO.
    total = sumItems(pItems).
    RETURN total.
  END METHOD.

  METHOD PRIVATE INTEGER sumItems(INPUT pCount AS INTEGER):
    RETURN pCount.
  END METHOD.
END CLASS.
