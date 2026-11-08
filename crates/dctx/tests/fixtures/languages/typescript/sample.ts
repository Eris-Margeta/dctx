@sealed
export class Boxed<T> {
    copy<U>(value: U): U {
        function nested<V>(item: V): V { return item; }
        return nested(value);
    }
}
export function café<T>(value: T): T { return value; }
