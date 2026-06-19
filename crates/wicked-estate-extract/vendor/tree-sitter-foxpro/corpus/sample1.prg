* Program: customer.prg
* Abstract: Customer maintenance — exercises bare PROCEDUREs, DEFINE CLASS with
*           PROCEDURE methods, function-call syntax, =func() statements, and && comments.
LPARAMETERS oAction

=InitApp()

DEFINE CLASS CustomerForm AS Form
	Caption = "Customer"

	PROCEDURE Init
		This.LoadDefaults()
		=SetupGrid(This)
	ENDPROC

	PROCEDURE cmdSave_Click
		IF ValidateForm(This)
			This.SaveRecord()
		ENDIF
	ENDPROC

	FUNCTION ComputeTotal(nQty, nPrice)
		RETURN nQty * nPrice
	ENDFUNC
ENDDEFINE

PROCEDURE InitApp
	SET TALK OFF
	OpenDatabase("crm")

PROCEDURE LogMessage
	LPARAMETERS cMsg
	=WriteLog(cMsg)
ENDPROC
