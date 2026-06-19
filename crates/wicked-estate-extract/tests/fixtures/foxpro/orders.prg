* orders.prg — order processing with a form class and standalone procedures.
DEFINE CLASS OrderForm AS Form
	Caption = "Orders"

	PROCEDURE Init
		This.LoadGrid()
		=RefreshTotals(This)
	ENDPROC

	FUNCTION LineTotal(nQty, nPrice)
		RETURN nQty
	ENDFUNC
ENDDEFINE

PROCEDURE PostOrder
	LPARAMETERS nOrderId
	IF ValidateOrder(nOrderId)
		=WriteLedger(nOrderId)
	ENDIF
ENDPROC
