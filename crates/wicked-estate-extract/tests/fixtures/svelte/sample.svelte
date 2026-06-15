<script>
  let count = 0;
  let message = "hello";

  function increment() {
    count += 1;
  }

  function reset() {
    count = 0;
    message = "reset";
  }
</script>

<ul>
  {#each Array(count) as _, i}
    <li>Item {i + 1}: {message}</li>
  {/each}
</ul>

<button on:click={increment}>Increment</button>
<button on:click={reset}>Reset</button>
