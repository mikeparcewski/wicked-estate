(ns geometry.core
  (:require [clojure.string :as str]
            [clojure.math :as math]))

(defn area
  "Compute the area of a shape map.
  Shape must have :kind and relevant dimension keys."
  [{:keys [kind radius width height a b c]}]
  (case kind
    :circle    (* math/PI radius radius)
    :rectangle (* width height)
    :triangle  (let [s (/ (+ a b c) 2.0)]
                 (math/sqrt (* s (- s a) (- s b) (- s c))))))

(defn perimeter
  "Compute the perimeter of a shape map."
  [{:keys [kind radius width height a b c]}]
  (case kind
    :circle    (* 2.0 math/PI radius)
    :rectangle (* 2.0 (+ width height))
    :triangle  (+ a b c)))

(defn describe
  "Return a human-readable description of a shape."
  [{:keys [kind radius width height a b c] :as shape}]
  (case kind
    :circle    (format "Circle(r=%.2f)" radius)
    :rectangle (format "Rectangle(%.2f x %.2f)" width height)
    :triangle  (format "Triangle(%.2f, %.2f, %.2f)" a b c)
    (str "Unknown shape: " shape)))

(defn largest
  "Return the shape with the maximum area from a collection."
  [shapes]
  (when (seq shapes)
    (apply max-key area shapes)))

(defn scale
  "Scale all dimensions of a shape by factor k."
  [k {:keys [kind radius width height a b c] :as shape}]
  (case kind
    :circle    (assoc shape :radius (* k radius))
    :rectangle (assoc shape :width (* k width) :height (* k height))
    :triangle  (assoc shape :a (* k a) :b (* k b) :c (* k c))))
