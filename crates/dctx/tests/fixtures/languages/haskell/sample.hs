module Sample where
data Boxed a = Boxed a
copy :: a -> a
copy value = nested value
  where nested item = item
café :: Int
café = 1
