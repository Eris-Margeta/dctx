@decorator
class Boxed:
    @staticmethod
    def copy(value):
        def nested(item):
            return item
        return nested(value)

def café(value: str) -> str:
    return value
