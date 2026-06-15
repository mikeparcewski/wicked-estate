#lang racket

(require racket/math)

;;; Shape record type
(define-record-type shape
  (make-shape kind params)
  shape?
  (kind   shape-kind)
  (params shape-params))

;;; Constructors
(define (make-circle r)
  (make-shape 'circle (list r)))

(define (make-rectangle w h)
  (make-shape 'rectangle (list w h)))

(define (make-triangle a b c)
  (make-shape 'triangle (list a b c)))

;;; Area — dispatches on kind symbol
(define (area s)
  (define params (shape-params s))
  (case (shape-kind s)
    [(circle)
     (let ([r (first params)])
       (* pi r r))]
    [(rectangle)
     (apply * params)]
    [(triangle)
     (let* ([a (first params)]
            [b (second params)]
            [c (third params)]
            [half (/ (+ a b c) 2.0)])
       (sqrt (* half (- half a) (- half b) (- half c))))]))

;;; Describe
(define (describe s)
  (define params (shape-params s))
  (case (shape-kind s)
    [(circle)    (format "Circle(r=~a)" (first params))]
    [(rectangle) (format "Rectangle(~a x ~a)" (first params) (second params))]
    [(triangle)  (format "Triangle(~a, ~a, ~a)"
                         (first params) (second params) (third params))]))

;;; Largest shape by area
(define (largest shapes)
  (foldl (lambda (s best)
           (if (>= (area s) (area best)) s best))
         (car shapes)
         (cdr shapes)))
