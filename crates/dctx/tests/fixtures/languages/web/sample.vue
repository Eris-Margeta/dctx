<template><h1>Fixture</h1></template>
<script lang="ts">
export class Boxed<T> {
    copy(value: T): T {
        function nested(item: T): T { return item; }
        return nested(value);
    }
}
function café() { return 1; }
</script>
