$PBExportHeader$n_cst_calc.sru
forward
global type n_cst_calc from nonvisualobject
end type
end forward

global type n_cst_calc from nonvisualobject
end type
global n_cst_calc n_cst_calc

forward prototypes
public function decimal of_linetotal (integer ai_qty, decimal adec_price)
public function integer of_post ()
end prototypes

public function decimal of_linetotal (integer ai_qty, decimal adec_price);
Return ai_qty * adec_price
end function

public function integer of_post ();
Decimal ldec_total
ldec_total = This.of_LineTotal(10, 2.50)
of_WriteLedger(ldec_total)
Return 1
end function
