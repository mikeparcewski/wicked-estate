# Informix 4GL — MAIN + FUNCTION with embedded SQL, CALL ... RETURNING, RUN, and a function call.
DATABASE life

GLOBALS
  DEFINE g_user CHAR(20)
END GLOBALS

MAIN
  DEFINE l_name CHAR(20)
  PROMPT "Enter Maiden Name: " FOR l_name
  CALL get_married(l_name) RETURNING l_name
  DISPLAY l_name
END MAIN

FUNCTION get_married(l_name)
  DEFINE l_name CHAR(20)
  DEFINE l_count INTEGER

  SELECT COUNT(*)
    INTO l_count
    FROM spouse
    WHERE name = l_name

  IF l_count > 0 THEN
    LET l_name = lookupSpouse(l_name)
  END IF

  RUN "logger.sh"
  RETURN l_name
END FUNCTION

REPORT summary(r_row)
  DEFINE r_row RECORD
    id INTEGER,
    name CHAR(20)
  END RECORD
  PRINT r_row.name
END REPORT
