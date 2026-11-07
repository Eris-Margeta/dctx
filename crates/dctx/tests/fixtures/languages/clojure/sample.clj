(ns sample)
(defmacro identity-value [value] value)
(defn copy [value]
  (letfn [(nested [item] item)]
    (nested value)))
(defn café [] 1)
(defrecord Boxed [value])
