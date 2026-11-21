#[macro_export]
macro_rules! performance {
  ($content:expr, $name:expr) => {{
    let start = ::std::time::Instant::now();
    let res = $content;
    ::std::println!("[{}] Time taken: {:?}", $name, start.elapsed());
    res
  }};
  ($content:expr) => {
    $crate::performance!($content, stringify!($content))
  };
}

#[cfg(test)]
mod tests {
  #[test]
  fn test_performance_macro() {
    let res = performance!(1 + 1);
    assert_eq!(res, 2);

    let res2 = performance!(2 * 3, "test_mult");
    assert_eq!(res2, 6);
  }
}
