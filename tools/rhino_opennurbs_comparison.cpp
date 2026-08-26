// SPDX-License-Identifier: Apache-2.0
#include "opennurbs_public.h"

#include <cstring>
#include <cstdlib>

int main(int argc, char** argv)
{
  if (argc != 4)
    return 2;
  const char* const fixture_kind = argv[1];
  if (std::strcmp(fixture_kind, "point") != 0 &&
      std::strcmp(fixture_kind, "structured") != 0)
    return 2;
  const int version = std::atoi(argv[2]);
  if (version != 50 && version != 60 && version != 70 && version != 80)
    return 2;

  ON::Begin();
  ONX_Model model;
  model.m_sStartSectionComments = "cadmpeg independent openNURBS comparison";
  model.AddDefaultLayer(nullptr, ON_Color::Black);
  if (std::strcmp(fixture_kind, "point") == 0)
  {
    ON_Point point(ON_3dPoint(1.25, -2.5, 4.0));
    model.AddModelGeometryComponent(&point, nullptr);
  }
  else
  {
    ON_Point point(ON_3dPoint(1.25, -2.5, 4.0));
    model.AddModelGeometryComponent(&point, nullptr);

    ON_LineCurve line(
      ON_3dPoint(-2.0, -1.0, 0.0),
      ON_3dPoint(2.0, -1.0, 0.0)
      );
    model.AddModelGeometryComponent(&line, nullptr);

    ON_Circle circle(ON_3dPoint(0.0, 2.0, 0.0), 1.5);
    ON_ArcCurve arc(circle, 0.0, ON_PI);
    model.AddModelGeometryComponent(&arc, nullptr);

    ON_3dPointArray polyline_points;
    polyline_points.Append(ON_3dPoint(-2.0, 4.0, 0.0));
    polyline_points.Append(ON_3dPoint(0.0, 5.0, 0.0));
    polyline_points.Append(ON_3dPoint(2.0, 4.0, 0.0));
    ON_PolylineCurve polyline(polyline_points);
    model.AddModelGeometryComponent(&polyline, nullptr);

    ON_Mesh mesh;
    mesh.m_V.Append(ON_3fPoint(-1.0f, 0.0f, 2.0f));
    mesh.m_V.Append(ON_3fPoint(1.0f, 0.0f, 2.0f));
    mesh.m_V.Append(ON_3fPoint(1.0f, 2.0f, 2.0f));
    mesh.m_V.Append(ON_3fPoint(-1.0f, 2.0f, 2.0f));
    ON_MeshFace face = ON_MeshFace::UnsetMeshFace;
    face.vi[0] = 0;
    face.vi[1] = 1;
    face.vi[2] = 2;
    face.vi[3] = 3;
    mesh.m_F.Append(face);
    model.AddModelGeometryComponent(&mesh, nullptr);

    const ON_3dPoint corners[8] = {
      ON_3dPoint(3.0, 0.0, 0.0),
      ON_3dPoint(5.0, 0.0, 0.0),
      ON_3dPoint(5.0, 2.0, 0.0),
      ON_3dPoint(3.0, 2.0, 0.0),
      ON_3dPoint(3.0, 0.0, 2.0),
      ON_3dPoint(5.0, 0.0, 2.0),
      ON_3dPoint(5.0, 2.0, 2.0),
      ON_3dPoint(3.0, 2.0, 2.0)
    };
    ON_Brep brep;
    if (ON_BrepBox(corners, &brep) == nullptr)
    {
      ON::End();
      return 1;
    }
    model.AddModelGeometryComponent(&brep, nullptr);
  }
  ON_TextLog log;
  const ON_wString output(argv[3]);
  const bool written = model.Write(output, version, &log);
  ON::End();
  return written ? 0 : 1;
}
