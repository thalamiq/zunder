import { CapabilityStatement } from "fhir/r4";
import { useState, useMemo } from "react";
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from "@thalamiq/ui/components/card";
import { Badge } from "@thalamiq/ui/components/badge";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@thalamiq/ui/components/collapsible";
import { ChevronDown } from "lucide-react";
import SearchInput from "./SearchInput";
import { PageHeader } from "./PageHeader";

interface CapabilityStatementProps {
  capabilityStatement: CapabilityStatement;
}

export const CapabilityStatementDisplay = ({
  capabilityStatement,
}: CapabilityStatementProps) => {
  const {
    name,
    title,
    version,
    description,
    software,
    implementation,
    rest,
    fhirVersion,
    format,
    status,
  } = capabilityStatement;

  const [resourceFilter, setResourceFilter] = useState("");

  // Filter function for resources
  const filterResources = (
    resources: NonNullable<NonNullable<typeof rest>[0]["resource"]>
  ) => {
    if (!resourceFilter) return resources;
    const filter = resourceFilter.toLowerCase();
    return resources.filter(
      (r) =>
        r.type?.toLowerCase().includes(filter) ||
        r.documentation?.toLowerCase().includes(filter)
    );
  };

  return (
    <div className="flex-1 space-y-4 overflow-y-auto p-6">
      <PageHeader
        title="Metadata"
        description={title || name || "Capability Statement"}
      />
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            {status && (
              <Badge variant={status === "active" ? "default" : "secondary"}>
                {status}
              </Badge>
            )}
          </div>
          {description && (
            <CardDescription className="mt-2">{description}</CardDescription>
          )}
        </CardHeader>
        <CardContent>
          <div className="flex flex-wrap gap-4 text-sm">
            {version && (
              <div>
                <span className="font-medium">Version:</span>{" "}
                <span className="text-muted-foreground">{version}</span>
              </div>
            )}
            {fhirVersion && (
              <div>
                <span className="font-medium">FHIR Version:</span>{" "}
                <span className="text-muted-foreground">{fhirVersion}</span>
              </div>
            )}
          </div>
        </CardContent>
      </Card>

      {/* Software & Implementation */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {software && (
          <Card>
            <CardHeader>
              <CardTitle>Software</CardTitle>
            </CardHeader>
            <CardContent className="space-y-1.5 text-sm">
              {software.name && (
                <div>
                  <span className="font-medium">Name:</span>{" "}
                  <span className="text-muted-foreground">{software.name}</span>
                </div>
              )}
              {software.version && (
                <div>
                  <span className="font-medium">Version:</span>{" "}
                  <span className="text-muted-foreground">
                    {software.version}
                  </span>
                </div>
              )}
              {software.releaseDate && (
                <div>
                  <span className="font-medium">Release Date:</span>{" "}
                  <span className="text-muted-foreground">
                    {software.releaseDate}
                  </span>
                </div>
              )}
            </CardContent>
          </Card>
        )}

        {implementation && (
          <Card>
            <CardHeader>
              <CardTitle>Implementation</CardTitle>
            </CardHeader>
            <CardContent className="space-y-1.5 text-sm">
              {implementation.description && (
                <div>
                  <span className="font-medium">Description:</span>{" "}
                  <span className="text-muted-foreground">
                    {implementation.description}
                  </span>
                </div>
              )}
              {implementation.url && (
                <div>
                  <span className="font-medium">URL:</span>{" "}
                  <a
                    href={implementation.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-primary hover:underline"
                  >
                    {implementation.url}
                  </a>
                </div>
              )}
            </CardContent>
          </Card>
        )}
      </div>

      {/* Formats */}
      {format && format.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>Supported Formats</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex flex-wrap gap-2">
              {format.map((fmt, idx) => (
                <Badge key={idx} variant="outline">
                  {fmt}
                </Badge>
              ))}
            </div>
          </CardContent>
        </Card>
      )}

      {/* REST Capabilities */}
      {rest && rest.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>REST Capabilities</CardTitle>
            {rest[0]?.documentation && (
              <CardDescription>{rest[0].documentation}</CardDescription>
            )}
          </CardHeader>
          <CardContent className="space-y-6">
            {rest.map((restCapability, idx) => (
              <div key={idx} className="space-y-4">
                {restCapability.mode && (
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium">Mode:</span>
                    <Badge variant="outline">{restCapability.mode}</Badge>
                  </div>
                )}

                {/* System Interactions */}
                {restCapability.interaction &&
                  restCapability.interaction.length > 0 && (
                    <div>
                      <h4 className="text-sm font-semibold mb-2">
                        System Interactions
                      </h4>
                      <div className="flex flex-wrap gap-2">
                        {restCapability.interaction.map((interaction, i) => (
                          <Badge key={i} variant="secondary">
                            {interaction.code}
                          </Badge>
                        ))}
                      </div>
                    </div>
                  )}

                {/* Resources */}
                {restCapability.resource &&
                  restCapability.resource.length > 0 &&
                  (() => {
                    const currentResources = restCapability.resource || [];
                    const filtered = filterResources(currentResources);
                    return (
                      <div>
                        <div className="flex items-center justify-between mb-3">
                          <h4 className="text-sm font-semibold">
                            Supported Resources ({filtered.length}
                            {filtered.length !== currentResources.length &&
                              ` of ${currentResources.length}`}
                            )
                          </h4>
                        </div>
                        <div className="mb-3">
                          <div className="relative">
                            <SearchInput
                              searchQuery={resourceFilter}
                              setSearchQuery={setResourceFilter}
                              placeholder="Filter resources..."
                            />
                          </div>
                        </div>
                        <div className="space-y-2">
                          {filtered.length === 0 ? (
                            <div className="text-sm text-muted-foreground text-center py-4">
                              No resources match your filter.
                            </div>
                          ) : (
                            filtered.map((resource, i) => (
                              <Collapsible key={i} defaultOpen>
                                <Card className="border-border/50 hover:border-border transition-colors">
                                  <CollapsibleTrigger className="w-full text-left">
                                    <CardHeader className="pb-3 px-4 py-3">
                                      <div className="flex items-center justify-between gap-3">
                                        <div className="flex items-center gap-2.5 min-w-0 flex-1">
                                          <ChevronDown className="h-4 w-4 text-muted-foreground transition-transform duration-200 shrink-0 group-data-[state=open]:rotate-180" />
                                          <CardTitle className="text-sm font-semibold truncate">
                                            {resource.type}
                                          </CardTitle>
                                        </div>
                                        {resource.profile && (
                                          <Badge
                                            variant="outline"
                                            className="text-xs shrink-0"
                                          >
                                            Profile
                                          </Badge>
                                        )}
                                      </div>
                                      {resource.documentation && (
                                        <CardDescription className="text-xs text-muted-foreground mt-1.5 line-clamp-2">
                                          {resource.documentation}
                                        </CardDescription>
                                      )}
                                    </CardHeader>
                                  </CollapsibleTrigger>
                                  <CollapsibleContent>
                                    <CardContent className="px-4 pb-4 pt-0 space-y-3">
                                      {resource.interaction &&
                                        resource.interaction.length > 0 && (
                                          <div className="space-y-1.5">
                                            <span className="text-xs font-medium text-muted-foreground">
                                              Interactions
                                            </span>
                                            <div className="flex flex-wrap gap-1.5">
                                              {resource.interaction.map(
                                                (interaction, j) => (
                                                  <Badge
                                                    key={j}
                                                    variant="secondary"
                                                    className="text-xs font-normal"
                                                  >
                                                    {interaction.code}
                                                  </Badge>
                                                )
                                              )}
                                            </div>
                                          </div>
                                        )}

                                      {resource.searchParam &&
                                        resource.searchParam.length > 0 && (
                                          <Collapsible>
                                            <CollapsibleTrigger className="text-xs font-medium text-muted-foreground hover:text-foreground transition-colors flex items-center gap-1.5 w-full">
                                              <span>
                                                Search Parameters ({resource.searchParam.length})
                                              </span>
                                              <ChevronDown className="h-3 w-3 transition-transform duration-200 group-data-[state=open]:rotate-180 shrink-0" />
                                            </CollapsibleTrigger>
                                            <CollapsibleContent>
                                              <div className="mt-2 space-y-2 pl-1">
                                                {resource.searchParam.map(
                                                  (param, j) => (
                                                    <div
                                                      key={j}
                                                      className="text-xs text-foreground/90"
                                                    >
                                                      <span className="font-medium">
                                                        {param.name}
                                                      </span>
                                                      {param.type && (
                                                        <span className="text-muted-foreground ml-1.5">
                                                          ({param.type})
                                                        </span>
                                                      )}
                                                      {param.documentation && (
                                                        <span className="text-muted-foreground block mt-0.5 ml-0">
                                                          {param.documentation}
                                                        </span>
                                                      )}
                                                    </div>
                                                  )
                                                )}
                                              </div>
                                            </CollapsibleContent>
                                          </Collapsible>
                                        )}
                                    </CardContent>
                                  </CollapsibleContent>
                                </Card>
                              </Collapsible>
                            ))
                          )}
                        </div>
                      </div>
                    );
                  })()}
              </div>
            ))}
          </CardContent>
        </Card>
      )}
    </div>
  );
};
