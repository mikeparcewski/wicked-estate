$PBExportHeader$pfc_n_cst_environment.sru
forward
global type pfc_n_cst_environment from n_base
end type
end forward

global type pfc_n_cst_environment from n_base autoinstantiate
event pfc_osversioninfodecode ( )
end type
global pfc_n_cst_environment pfc_n_cst_environment

type variables
Protected:
Boolean ib_HaveValues = false
Public:
String is_OSSummaryDesc
end variables

forward prototypes
public function integer of_refresh ()
protected function integer of_getosinfo ()
end prototypes

event pfc_osversioninfodecode();
Integer li_RC
li_RC = This.of_GetOSInfo()
end event

public function integer of_refresh ();
ULong lul_rc
is_OSSummaryDesc = ''
If of_GetEnvironment() <> 1 Then Return -1
lul_rc = GetComputerName(is_ComputerName)
Return 1
end function

protected function integer of_getosinfo ();
This.EVENT pfc_OSVersionInfoDecode()
Return 1
end function
