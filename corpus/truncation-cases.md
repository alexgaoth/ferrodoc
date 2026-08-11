Mid-document truncation battery (regression material for trailing-newline rules).

1. <!-- comment truncated by sibling item
2. next item

3. ```
   fence truncated by sibling
4. next item

- ```
  fence truncated by blank + list end

paragraph after the list

- <!-- comment truncated by blank + list end

another paragraph

> ```
> fence truncated inside quote

> <!-- comment inside quote

<!-- top-level comment that swallows

everything after a blank line -->

done
