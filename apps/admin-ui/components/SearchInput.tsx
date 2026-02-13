import { Input } from "@thalamiq/ui/components/input";
import { Search, X } from "lucide-react";
import { cn } from "@thalamiq/ui/utils";

interface SearchInputProps {
  searchQuery: string;
  setSearchQuery: (searchQuery: string) => void;
  placeholder?: string;
  inputClassName?: string;
}

const SearchInput = ({
  searchQuery,
  setSearchQuery,
  placeholder = "Search...",
  inputClassName,
}: SearchInputProps) => {
  return (
    <div className="relative flex-1">
      <Search className="h-4 w-4 text-muted-foreground absolute left-3 top-1/2 transform -translate-y-1/2" />
      <Input
        type="text"
        value={searchQuery}
        onChange={(e) => setSearchQuery(e.target.value)}
        className={cn("pl-10", inputClassName)}
        placeholder={placeholder}
        spellCheck={false}
      />
      {searchQuery && (
        <button
          type="button"
          onClick={() => setSearchQuery("")}
          className="absolute right-3 top-1/2 transform -translate-y-1/2 p-1 hover:bg-muted rounded-sm transition-colors z-10"
        >
          <X className="h-4 w-4 text-muted-foreground hover:text-foreground" />
        </button>
      )}
    </div>
  );
};

export default SearchInput;
