import "./styles.css";
import { mountApp } from "./ui";

const root = document.getElementById("app");
if (root) {
  mountApp(root);
}
