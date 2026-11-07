/** A container. */
export class Boxed {
    copy(value) {
        function nested(item) { return item; }
        return nested(value);
    }
}
export function café(value) { return value; }
const arrow = (value) => value;
