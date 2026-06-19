# Informix 4GL billing module — MAIN driver + FUNCTION with embedded SQL and calls.
MAIN
  DEFINE l_acct INTEGER
  LET l_acct = 1001
  CALL post_invoice(l_acct) RETURNING l_acct
END MAIN

FUNCTION post_invoice(p_acct)
  DEFINE p_acct INTEGER
  DEFINE l_total DECIMAL(10,2)

  SELECT SUM(amount)
    INTO l_total
    FROM charges
    WHERE acct_id = p_acct

  CALL write_ledger(p_acct, l_total)
  RETURN l_total
END FUNCTION

FUNCTION write_ledger(p_acct, p_amt)
  DEFINE p_acct INTEGER
  DEFINE p_amt DECIMAL(10,2)
  DISPLAY "posted"
END FUNCTION
