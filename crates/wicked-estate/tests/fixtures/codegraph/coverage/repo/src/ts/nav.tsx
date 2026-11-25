import { Link, Route, Routes } from "react-router-dom";

export function About() {
  return <p>about</p>;
}

export function Nav() {
  return <Link to="/about">About</Link>;
}

export function App() {
  return (
    <Routes>
      <Route path="/about" element={<About />} />
    </Routes>
  );
}
