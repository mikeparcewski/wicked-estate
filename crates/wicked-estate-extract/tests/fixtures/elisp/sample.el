(defvar *counter* 0 "Global counter.")
(defvar *prefix* "item" "Prefix for display.")

(defun increment-counter ()
  "Increment the global counter by one."
  (setq *counter* (1+ *counter*)))

(defun reset-counter ()
  "Reset the global counter to zero."
  (setq *counter* 0))

(defun display-counter ()
  "Display the current counter value."
  (message "%s-%d" *prefix* *counter*))

(defmacro with-counter-reset (&rest body)
  "Execute BODY then reset the counter."
  `(progn
     ,@body
     (reset-counter)))

(with-counter-reset
  (increment-counter)
  (increment-counter)
  (display-counter))
