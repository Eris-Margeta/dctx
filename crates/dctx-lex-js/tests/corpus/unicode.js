// Unicode, ASI and nesting. Second sentence.
const café = (naïve) => naïve * 2
const 数据 = { 值: () => café(1) }
let total = 0
total++
if (/regex-at-start/.test('x')) run()
function ünïcode() {
  return `outer ${`inner ${'deep'.repeat(2)} ${(() => café(3))()}`}`
}
const divide = (a, b) => a / b / 2
const template = `a/${divide(4, 2)}/b`
label: for (const x of [1, 2]) {
  if (x > 1) break label
}
export { café as cafe, ünïcode, 数据, template }
