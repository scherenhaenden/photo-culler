"""Rich console output renderer and printer."""

from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.progress import Progress, SpinnerColumn, TextColumn, BarColumn, TaskProgressColumn
from typing import Any, List, Dict, Optional


class OutputRenderer:
    """Wrapper around Rich Console for formatted CLI output."""

    def __init__(self, no_color: bool = False, quiet: bool = False):
        self.console = Console(no_color=no_color, quiet=quiet)

    def print(self, message: str, style: Optional[str] = None):
        self.console.print(message, style=style)

    def success(self, message: str):
        self.console.print(f"[bold green]✔[/bold green] {message}")

    def warning(self, message: str):
        self.console.print(f"[bold yellow]⚠[/bold yellow] {message}")

    def error(self, message: str):
        self.console.print(f"[bold red]✖[/bold red] {message}")

    def panel(self, content: str, title: str = "", subtitle: str = ""):
        self.console.print(Panel(content, title=title, subtitle=subtitle, border_style="blue"))

    def render_table(self, title: str, headers: List[str], rows: List[List[Any]]):
        table = Table(title=title, show_header=True, header_style="bold cyan")
        for h in headers:
            table.add_column(h)
        for r in rows:
            table.add_row(*[str(cell) for cell in r])
        self.console.print(table)
