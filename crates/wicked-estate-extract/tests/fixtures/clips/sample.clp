; CLIPS/Jess sample — wicked-estate W15.7 fixture.
; Demonstrates: defmodule, deftemplate, defrule with LHS conditions and RHS actions.

(defmodule MAIN (export ?ALL))

; Fact templates (working-memory types).
(deftemplate person
  (slot name (type STRING))
  (slot age (type INTEGER))
  (slot status (type SYMBOL) (default unknown)))

(deftemplate adult
  (slot person-name (type STRING)))

; Rule 1 — classify adults.
(defrule adult-check "Check if a person is 18 or older"
  ?p <- (person (name ?name) (age ?age))
  (test (>= ?age 18))
  =>
  (assert (adult (person-name ?name)))
  (printout t "Adult found: " ?name crlf))

; Rule 2 — classify minors.
(defrule minor-check "Check if a person is under 18"
  ?p <- (person (name ?name) (age ?age))
  (test (< ?age 18))
  =>
  (assert (status-update ?name minor))
  (printout t "Minor found: " ?name crlf))

; Rule 3 — greet everyone.
(defrule greet-all "Greet every known person"
  (person (name ?name))
  =>
  (printout t "Hello, " ?name "!" crlf))

; Initial facts.
(deffacts initial-population
  (person (name "Alice") (age 30))
  (person (name "Bob")   (age 15))
  (person (name "Carol") (age 22)))
