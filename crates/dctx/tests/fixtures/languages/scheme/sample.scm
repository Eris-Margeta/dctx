(define-syntax identity-value
  (syntax-rules () ((_ value) value)))
(define (copy-value value)
  (define (nested item) item)
  (nested value))
(define (café) 1)
