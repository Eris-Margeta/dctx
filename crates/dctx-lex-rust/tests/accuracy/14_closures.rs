fn pipeline() {
    let double = |value: u32| value * 2;
    let next = async move || 3;
    let run = || { fn local() {} local(); };
}
