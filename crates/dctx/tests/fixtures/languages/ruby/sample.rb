module Sample
  class Boxed
    def copy(value)
      def nested(item)
        item
      end
      nested(value)
    end
    def café
      1
    end
  end
end
