(defpackage :geometry
  (:use :cl)
  (:export :shape :make-circle :make-rectangle :make-triangle
           :area :perimeter :describe-shape))

(in-package :geometry)

(defstruct shape
  "A geometric shape with a kind keyword and dimension list."
  (kind   :circle  :type keyword)
  (params '()      :type list))

(defun make-circle (r)
  "Construct a circle shape with radius R."
  (make-shape :kind :circle :params (list r)))

(defun make-rectangle (w h)
  "Construct a rectangle shape with width W and height H."
  (make-shape :kind :rectangle :params (list w h)))

(defun make-triangle (a b c)
  "Construct a triangle shape with sides A, B, C."
  (make-shape :kind :triangle :params (list a b c)))

(defun area (s)
  "Compute the area of shape S."
  (let ((params (shape-params s)))
    (ecase (shape-kind s)
      (:circle
       (let ((r (first params)))
         (* pi r r)))
      (:rectangle
       (destructuring-bind (w h) params (* w h)))
      (:triangle
       (destructuring-bind (a b c) params
         (let ((half-perim (/ (+ a b c) 2.0)))
           (sqrt (* half-perim
                    (- half-perim a)
                    (- half-perim b)
                    (- half-perim c)))))))))

(defun perimeter (s)
  "Compute the perimeter of shape S."
  (let ((params (shape-params s)))
    (ecase (shape-kind s)
      (:circle    (* 2.0 pi (first params)))
      (:rectangle (destructuring-bind (w h) params (* 2.0 (+ w h))))
      (:triangle  (apply #'+ params)))))

(defun describe-shape (s)
  "Return a descriptive string for shape S."
  (let ((params (shape-params s)))
    (ecase (shape-kind s)
      (:circle    (format nil "Circle(r=~,2f)" (first params)))
      (:rectangle (format nil "Rectangle(~,2f x ~,2f)" (first params) (second params)))
      (:triangle  (format nil "Triangle(~{~,2f~^, ~})" params)))))
